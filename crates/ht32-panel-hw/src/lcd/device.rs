//! LCD device communication via USB HID.

use crate::orientation::Orientation;
use crate::{Error, Result, LCD_PID, LCD_VID};
use hidapi::{HidApi, HidDevice};
use std::sync::Mutex;
use tracing::{debug, info};

use super::framebuffer::Framebuffer;
use super::protocol::{
    build_heartbeat_packet, build_orientation_packet, build_redraw_chunk, build_refresh_packet,
    CHUNK_COUNT, DATA_SIZE,
};

/// A sub-tile produced by splitting a large rect to satisfy 0xA2 packet limits.
pub struct SubTile {
    pub x: u16,
    pub y: u16,
    pub w: u8,
    pub h: u8,
    pub pixels: Vec<u16>,
}

/// Splits a `w×h` rect (row-major `pixels`, stride=`w`) into sub-tiles satisfying
/// the 0xA2 constraints: width ≤ 255, height ≤ 255, and pixel count ≤ 2048.
pub fn tile_rect(x: u16, y: u16, w: u16, h: u16, pixels: &[u16]) -> Vec<SubTile> {
    let mut tiles = Vec::new();
    let mut col_x = x;
    let mut col_remaining = w;

    while col_remaining > 0 {
        let cw = col_remaining.min(255) as u8;
        // max rows such that cw * max_h ≤ 2048, capped at 255
        let max_h = ((2048 / cw as usize) as u16).min(255) as u8;

        let mut row_y = y;
        let mut row_remaining = h;
        while row_remaining > 0 {
            let ch = row_remaining.min(max_h as u16) as u8;

            // Extract pixel slice from row-major source with stride = w
            let col_offset = (col_x - x) as usize;
            let row_offset = (row_y - y) as usize;
            let mut tile_pixels = Vec::with_capacity(cw as usize * ch as usize);
            for row in 0..ch as usize {
                let src_row_start = (row_offset + row) * w as usize + col_offset;
                tile_pixels.extend_from_slice(&pixels[src_row_start..src_row_start + cw as usize]);
            }

            tiles.push(SubTile {
                x: col_x,
                y: row_y,
                w: cw,
                h: ch,
                pixels: tile_pixels,
            });

            row_y += ch as u16;
            row_remaining -= ch as u16;
        }

        col_x += cw as u16;
        col_remaining -= cw as u16;
    }

    tiles
}

/// LCD device controller.
pub struct LcdDevice {
    device: Mutex<HidDevice>,
    current_orientation: Mutex<Orientation>,
    /// The previously transmitted (post-rotation, device-space) framebuffer bytes,
    /// used by `redraw_diff` to compute the tile-diff. `None` until the first send.
    prev_transmitted: Mutex<Option<Vec<u16>>>,
}

/// Tile width for `redraw_diff`'s diff grid (device space). 80×16 = 1280 px ≤ 2048,
/// and both dims ≤ 255, so each tile fits a single 0xA2 packet.
const TILE_W: u16 = 80;
/// Tile height for `redraw_diff`'s diff grid (device space).
const TILE_H: u16 = 16;

/// Pure transform mirroring what `redraw`/`redraw_diff` place on the wire: a copy of
/// `data`, rotated 180° iff `rotate` is set. Kept free-standing so it can be unit-tested
/// without a device.
fn transmit_transform(data: &[u16], width: u16, height: u16, rotate: bool) -> Vec<u16> {
    let mut out = data.to_vec();
    if rotate {
        Orientation::rotate_180(&mut out, width, height);
    }
    out
}

/// Walks a `tile_w×tile_h` grid over the `fb_w×fb_h` image (row-major, stride `fb_w`),
/// clipping the last row/column to the frame edge. Emits `(x, y, w, h)` for every tile
/// whose `prev` and `new` pixel sub-rects differ. Pure: this is the diff core.
pub fn diff_tiles(
    prev: &[u16],
    new: &[u16],
    fb_w: u16,
    fb_h: u16,
    tile_w: u16,
    tile_h: u16,
) -> Vec<(u16, u16, u16, u16)> {
    let mut changed = Vec::new();
    let stride = fb_w as usize;

    let mut ty = 0u16;
    while ty < fb_h {
        let th = tile_h.min(fb_h - ty);
        let mut tx = 0u16;
        while tx < fb_w {
            let tw = tile_w.min(fb_w - tx);

            let mut differs = false;
            'rows: for row in 0..th as usize {
                let row_start = (ty as usize + row) * stride + tx as usize;
                let prev_row = &prev[row_start..row_start + tw as usize];
                let new_row = &new[row_start..row_start + tw as usize];
                if prev_row != new_row {
                    differs = true;
                    break 'rows;
                }
            }

            if differs {
                changed.push((tx, ty, tw, th));
            }

            tx += tile_w;
        }
        ty += tile_h;
    }

    changed
}

/// Decides whether `redraw_diff` must take the full-redraw path: when forced, when
/// there is no previous frame, or when the previous frame's length differs (an
/// orientation/size change invalidates the tile diff). Pure, so the branch is testable
/// without a device.
fn needs_full_redraw(force_full: bool, prev_len: Option<usize>, transmitted_len: usize) -> bool {
    force_full || prev_len.is_none_or(|len| len != transmitted_len)
}

/// Extracts a tile's row-major pixels from a `stride`-wide source buffer.
fn extract_tile(src: &[u16], stride: usize, x: u16, y: u16, w: u16, h: u16) -> Vec<u16> {
    let mut tile = Vec::with_capacity(w as usize * h as usize);
    for row in 0..h as usize {
        let row_start = (y as usize + row) * stride + x as usize;
        tile.extend_from_slice(&src[row_start..row_start + w as usize]);
    }
    tile
}

/// The HID interface number used for LCD data transfer.
/// The device has multiple interfaces; interface 1 is for display data.
/// Reference implementation uses interface 1 (path "1-8:1.1").
const LCD_INTERFACE: i32 = 1;

impl LcdDevice {
    /// Opens the LCD device by VID:PID.
    ///
    /// The device has multiple HID interfaces. This function finds and opens
    /// the correct interface for display control (interface 1).
    pub fn open() -> Result<Self> {
        let api = HidApi::new()?;

        // Enumerate all devices to find the correct interface
        let devices: Vec<_> = api
            .device_list()
            .filter(|d| d.vendor_id() == LCD_VID && d.product_id() == LCD_PID)
            .collect();

        if devices.is_empty() {
            return Err(Error::LcdNotFound);
        }

        // Log all found interfaces for debugging
        for dev in &devices {
            debug!(
                "Found HID device: path={:?}, interface={}",
                dev.path(),
                dev.interface_number()
            );
        }

        // The display data interface is interface 1 (output-only, libusb-backed):
        // the kernel creates no hidraw node for it, so it must be opened directly.
        // Interface 0 is the consumer-control input device; never write display
        // data there, so do not fall back to an arbitrary interface.
        let device_info = devices
            .iter()
            .find(|d| d.interface_number() == LCD_INTERFACE)
            .ok_or(Error::LcdNotFound)?;

        let device = device_info.open_device(&api).map_err(|e| {
            debug!("Failed to open device: {}", e);
            Error::LcdNotFound
        })?;

        info!(
            "LCD device opened (VID:{:04X} PID:{:04X}, interface={})",
            LCD_VID,
            LCD_PID,
            device_info.interface_number()
        );

        // Initial cooldown period - device needs time to initialize after opening.
        // Reference implementation uses 1000ms delay before any commands.
        // Uses blocking sleep since this only runs once at startup/reconnection.
        debug!("Waiting for device initialization (1s cooldown)...");
        std::thread::sleep(std::time::Duration::from_millis(1000));

        Ok(Self {
            device: Mutex::new(device),
            current_orientation: Mutex::new(Orientation::default()),
            prev_transmitted: Mutex::new(None),
        })
    }

    /// Opens a specific LCD device by path.
    pub fn open_path(path: &str) -> Result<Self> {
        let api = HidApi::new()?;

        let c_path = std::ffi::CString::new(path).map_err(|_| {
            Error::SerialIo(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid device path: {}", path),
            ))
        })?;
        let device = api
            .open_path(c_path.as_c_str())
            .map_err(|_| Error::LcdNotFound)?;

        info!("LCD device opened at path: {}", path);

        Ok(Self {
            device: Mutex::new(device),
            current_orientation: Mutex::new(Orientation::default()),
            prev_transmitted: Mutex::new(None),
        })
    }

    /// Sets the display orientation.
    pub fn set_orientation(&self, orientation: Orientation) -> Result<()> {
        let packet = build_orientation_packet(orientation.is_portrait());

        debug!("Orientation packet header: {:02X?}", &packet[0..10]);

        let device = self.device.lock().unwrap();
        device.write(&packet)?;

        *self.current_orientation.lock().unwrap() = orientation;
        debug!("Set orientation to {}", orientation);

        Ok(())
    }

    /// Gets the current orientation.
    pub fn orientation(&self) -> Orientation {
        *self.current_orientation.lock().unwrap()
    }

    /// Sends a heartbeat with explicit time values.
    pub fn heartbeat_with_time(&self, hours: u8, minutes: u8, seconds: u8) -> Result<()> {
        let packet = build_heartbeat_packet(hours, minutes, seconds);

        debug!("Heartbeat packet header: {:02X?}", &packet[0..10]);

        let device = self.device.lock().unwrap();
        device.write(&packet)?;
        debug!("Heartbeat sent: {:02}:{:02}:{:02}", hours, minutes, seconds);

        Ok(())
    }

    /// Computes the exact bytes `redraw` places on the wire: a copy of the
    /// framebuffer data with the same 180° rotation `redraw` applies (iff the current
    /// orientation needs it). Shared by `redraw` and `redraw_diff` so they cannot drift.
    fn transmitted_bytes(&self, framebuffer: &Framebuffer) -> Vec<u16> {
        let rotate = self.current_orientation.lock().unwrap().needs_rotation();
        transmit_transform(
            framebuffer.data(),
            framebuffer.width(),
            framebuffer.height(),
            rotate,
        )
    }

    /// Sends the full frame as the 27 `0xA3` chunks. `transmitted` is already in
    /// device space (post-rotation). Shared by `redraw` and `redraw_diff`'s full path.
    fn send_full(&self, transmitted: &[u16]) -> Result<()> {
        let device = self.device.lock().unwrap();

        for chunk_idx in 0..CHUNK_COUNT {
            let offset = chunk_idx * (DATA_SIZE / 2);
            let packet = build_redraw_chunk(chunk_idx, transmitted, offset);

            // Log first chunk header for debugging
            if chunk_idx == 0 {
                debug!(
                    "Redraw chunk 0 header: {:02X?}, first data bytes: {:02X?}",
                    &packet[0..12],
                    &packet[9..21]
                );
            }

            device.write(&packet)?;
        }

        debug!("Full redraw completed ({} chunks)", CHUNK_COUNT);
        Ok(())
    }

    /// Performs a full screen redraw.
    pub fn redraw(&self, framebuffer: &Framebuffer) -> Result<()> {
        let transmitted = self.transmitted_bytes(framebuffer);
        self.send_full(&transmitted)
    }

    /// Transmits only the pixels that changed versus the previously transmitted frame.
    ///
    /// Diffs the new frame (in device space, post-rotation) against the stored previous
    /// frame in a fixed tile grid and sends each changed tile via the no-rotation 0xA2
    /// partial write. Falls back to a full redraw when `force_full` is set, on the first
    /// frame, or when the transmitted length changes (orientation/size change).
    ///
    /// Returns the number of changed tiles sent, or `usize::MAX` as a sentinel when the
    /// full-redraw path was taken.
    pub fn redraw_diff(&self, framebuffer: &Framebuffer, force_full: bool) -> Result<usize> {
        let transmitted = self.transmitted_bytes(framebuffer);
        let fb_w = framebuffer.width();
        let fb_h = framebuffer.height();

        let mut prev = self.prev_transmitted.lock().unwrap();

        let full = needs_full_redraw(
            force_full,
            prev.as_ref().map(|p| p.len()),
            transmitted.len(),
        );

        if full {
            self.send_full(&transmitted)?;
            *prev = Some(transmitted);
            return Ok(usize::MAX);
        }

        let prev_bytes = prev.as_ref().expect("prev is Some on the diff path");
        let tiles = diff_tiles(prev_bytes, &transmitted, fb_w, fb_h, TILE_W, TILE_H);
        let stride = fb_w as usize;

        for &(tx, ty, tw, th) in &tiles {
            let tile_pixels = extract_tile(&transmitted, stride, tx, ty, tw, th);
            self.refresh_raw(tx, ty, tw, th, &tile_pixels)?;
        }

        debug!("redraw_diff sent {} changed tiles", tiles.len());
        *prev = Some(transmitted);
        Ok(tiles.len())
    }

    /// Performs a partial refresh of a rectangular region.
    pub fn refresh(&self, x: u16, y: u16, width: u8, height: u8, pixels: &[u16]) -> Result<()> {
        let orientation = *self.current_orientation.lock().unwrap();
        let mut data = pixels.to_vec();

        // Apply software rotation if needed
        if orientation.needs_rotation() {
            Orientation::rotate_180(&mut data, width as u16, height as u16);
        }

        let packet = build_refresh_packet(x, y, width, height, &data);

        let device = self.device.lock().unwrap();
        device.write(&packet)?;

        debug!("Partial refresh at ({}, {}) {}x{}", x, y, width, height);
        Ok(())
    }

    /// Sends a rectangle of already-final (post-rotation, device-space) pixels with
    /// no rotation applied, sub-tiling as needed to satisfy 0xA2 packet limits.
    pub fn refresh_raw(
        &self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        pixels: &[u16],
    ) -> Result<()> {
        let sub_tiles = tile_rect(x, y, width, height, pixels);
        let device = self.device.lock().unwrap();
        for st in &sub_tiles {
            let packet = build_refresh_packet(st.x, st.y, st.w, st.h, &st.pixels);
            device.write(&packet)?;
            debug!(
                "Partial raw write at ({}, {}) {}x{}",
                st.x, st.y, st.w, st.h
            );
        }
        Ok(())
    }

    /// Clears the display to a solid color.
    pub fn clear(&self, color: u16) -> Result<()> {
        let mut fb = Framebuffer::new();
        fb.clear(color);
        self.redraw(&fb)
    }

    /// Sends a heartbeat to keep the device alive using system time.
    pub fn heartbeat(&self) -> Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};

        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = duration.as_secs();

        // Simple time extraction (UTC)
        let hours = ((secs / 3600) % 24) as u8;
        let minutes = ((secs / 60) % 60) as u8;
        let seconds = (secs % 60) as u8;

        self.heartbeat_with_time(hours, minutes, seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hardware tests are skipped by default
    #[test]
    #[ignore]
    fn test_device_open() {
        let device = LcdDevice::open();
        assert!(device.is_ok());
    }

    fn check_invariants(tiles: &[SubTile], total_w: u16, total_h: u16) {
        for t in tiles {
            assert!(t.w as usize <= 255, "w > 255: {}", t.w);
            assert!(t.h as usize <= 255, "h > 255: {}", t.h);
            let px = t.w as usize * t.h as usize;
            assert!(px <= 2048, "pixel count {} > 2048", px);
            assert_eq!(t.pixels.len(), px);
        }
        let total_pixels: usize = tiles.iter().map(|t| t.w as usize * t.h as usize).sum();
        assert_eq!(
            total_pixels,
            total_w as usize * total_h as usize,
            "total pixels mismatch"
        );
    }

    #[test]
    fn tile_rect_304x4_splits_into_two_columns() {
        let w: u16 = 304;
        let h: u16 = 4;
        let pixels: Vec<u16> = (0..w * h).collect();
        let tiles = tile_rect(0, 0, w, h, &pixels);

        // 304 > 255 → col0: w=255, col1: w=49; each col: 255*4=1020 ≤ 2048 → 1 row chunk each
        assert_eq!(tiles.len(), 2, "expected 2 sub-tiles");

        assert_eq!(tiles[0].x, 0);
        assert_eq!(tiles[0].y, 0);
        assert_eq!(tiles[0].w, 255);
        assert_eq!(tiles[0].h, 4);

        assert_eq!(tiles[1].x, 255);
        assert_eq!(tiles[1].y, 0);
        assert_eq!(tiles[1].w, 49);
        assert_eq!(tiles[1].h, 4);

        // Pixel at row=0, col=0 in source → tiles[0].pixels[0]
        assert_eq!(tiles[0].pixels[0], pixels[0]);
        // Pixel at row=0, col=254 in source → tiles[0].pixels[254]
        assert_eq!(tiles[0].pixels[254], pixels[254]);
        // Pixel at row=0, col=255 in source → tiles[1].pixels[0]
        assert_eq!(tiles[1].pixels[0], pixels[255]);
        // Pixel at row=2, col=300 → source index 2*304+300=908 → tiles[1] row=2, col=300-255=45 → index 2*49+45=143
        assert_eq!(tiles[1].pixels[143], pixels[2 * 304 + 300]);

        check_invariants(&tiles, w, h);
    }

    #[test]
    fn tile_rect_width_255_exact() {
        let w: u16 = 255;
        let h: u16 = 1;
        let pixels: Vec<u16> = (0..w * h).collect();
        let tiles = tile_rect(0, 0, w, h, &pixels);

        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].w, 255);
        assert_eq!(tiles[0].h, 1);
        check_invariants(&tiles, w, h);
    }

    #[test]
    fn tile_rect_width_256_two_columns() {
        let w: u16 = 256;
        let h: u16 = 1;
        let pixels: Vec<u16> = (0..w * h).collect();
        let tiles = tile_rect(0, 0, w, h, &pixels);

        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].w, 255);
        assert_eq!(tiles[0].h, 1);
        assert_eq!(tiles[1].w, 1);
        assert_eq!(tiles[1].h, 1);
        check_invariants(&tiles, w, h);
    }

    #[test]
    fn tile_rect_pixel_count_exceeds_2048_splits_by_height() {
        // 64*33 = 2112 > 2048; max_h = 2048/64 = 32 → 2 row chunks (32 + 1)
        let w: u16 = 64;
        let h: u16 = 33;
        let pixels: Vec<u16> = (0..w * h).collect();
        let tiles = tile_rect(0, 0, w, h, &pixels);

        // 1 column (64 ≤ 255), 2 row chunks
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].w, 64);
        assert_eq!(tiles[0].h, 32);
        assert_eq!(tiles[1].w, 64);
        assert_eq!(tiles[1].h, 1);
        check_invariants(&tiles, w, h);
    }

    #[test]
    fn tile_rect_one_by_one() {
        let tiles = tile_rect(5, 10, 1, 1, &[0xABCD]);
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].x, 5);
        assert_eq!(tiles[0].y, 10);
        assert_eq!(tiles[0].w, 1);
        assert_eq!(tiles[0].h, 1);
        assert_eq!(tiles[0].pixels, vec![0xABCD]);
        check_invariants(&tiles, 1, 1);
    }

    #[test]
    fn tile_rect_nonzero_origin() {
        // Origin offset is preserved in sub-tile coordinates
        let w: u16 = 10;
        let h: u16 = 5;
        let pixels: Vec<u16> = (100..w * h + 100).collect();
        let tiles = tile_rect(20, 30, w, h, &pixels);

        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].x, 20);
        assert_eq!(tiles[0].y, 30);
        assert_eq!(tiles[0].w, 10);
        assert_eq!(tiles[0].h, 5);
        assert_eq!(tiles[0].pixels, pixels);
        check_invariants(&tiles, w, h);
    }

    // --- transmit_transform (the redraw transmitted-bytes core) ---

    #[test]
    fn transmit_transform_landscape_is_identity() {
        let data: Vec<u16> = (0..(320u32 * 170) as u16).collect();
        let out = transmit_transform(&data, 320, 170, false);
        assert_eq!(out, data, "no-rotation output must equal framebuffer data");
    }

    #[test]
    fn transmit_transform_upside_down_is_rotate_180() {
        let data: Vec<u16> = (0..(320u32 * 170) as u16).collect();
        let out = transmit_transform(&data, 320, 170, true);

        // Manual expected: rotate_180 == full reverse for a row-major buffer.
        let mut expected = data.clone();
        expected.reverse();
        assert_eq!(out, expected, "rotation output must equal rotate_180(data)");
        assert_ne!(out, data, "rotation must actually change the data");
    }

    // --- diff_tiles (the main correctness target) ---

    const FB_W: u16 = 320;
    const FB_H: u16 = 170;

    fn blank() -> Vec<u16> {
        vec![0u16; FB_W as usize * FB_H as usize]
    }

    fn idx(x: u16, y: u16) -> usize {
        y as usize * FB_W as usize + x as usize
    }

    #[test]
    fn diff_tiles_identical_is_empty() {
        // THE "zero send on unchanged" property.
        let prev = blank();
        let new = prev.clone();
        let tiles = diff_tiles(&prev, &new, FB_W, FB_H, TILE_W, TILE_H);
        assert!(tiles.is_empty(), "identical frames must send zero tiles");
    }

    #[test]
    fn diff_tiles_single_pixel_one_tile() {
        let prev = blank();
        let mut new = blank();
        new[idx(100, 50)] = 0xBEEF;

        let tiles = diff_tiles(&prev, &new, FB_W, FB_H, TILE_W, TILE_H);
        assert_eq!(tiles.len(), 1, "one pixel must dirty exactly one tile");

        // (100,50): tile col = 100/80 = 1 → x=80,w=80; tile row = 50/16 = 3 → y=48,h=16.
        assert_eq!(tiles[0], (80, 48, 80, 16));
    }

    #[test]
    fn diff_tiles_block_spanning_boundary() {
        // A 10×10 block at (75,45) straddles x boundary 80 (cols 0,1) and y boundary 48
        // (rows 2,3) → exactly 4 overlapping tiles.
        let prev = blank();
        let mut new = blank();
        for dy in 0..10u16 {
            for dx in 0..10u16 {
                new[idx(75 + dx, 45 + dy)] = 0xF00D;
            }
        }

        let mut tiles = diff_tiles(&prev, &new, FB_W, FB_H, TILE_W, TILE_H);
        tiles.sort();
        assert_eq!(
            tiles,
            vec![
                (0, 32, 80, 16),
                (0, 48, 80, 16),
                (80, 32, 80, 16),
                (80, 48, 80, 16)
            ],
            "10x10 block across both boundaries → exactly 4 tiles"
        );
    }

    #[test]
    fn diff_tiles_clipped_last_row() {
        // Change at y=168 with fb_h=170, tile_h=16: row index 168/16 = 10 → y=160, but the
        // grid clips to the frame edge so h = 170 - 160 = 10, not 16.
        let prev = blank();
        let mut new = blank();
        new[idx(8, 168)] = 0x1234;

        let tiles = diff_tiles(&prev, &new, FB_W, FB_H, TILE_W, TILE_H);
        assert_eq!(tiles.len(), 1);
        assert_eq!(
            tiles[0],
            (0, 160, 80, 10),
            "last row tile must clip h to 10"
        );
    }

    #[test]
    fn diff_tiles_full_change_tiles_whole_frame() {
        // prev zeros, new all-ones → every grid tile changes, and the tiles must tile the
        // whole 320×170 image with no gaps and no overlaps.
        let prev = blank();
        let new = vec![1u16; FB_W as usize * FB_H as usize];

        let tiles = diff_tiles(&prev, &new, FB_W, FB_H, TILE_W, TILE_H);

        // 320/80 = 4 cols, ceil(170/16) = 11 rows → 44 tiles.
        assert_eq!(tiles.len(), 44, "full change must emit every grid tile");

        // Total area covers exactly the frame.
        let total: usize = tiles
            .iter()
            .map(|&(_, _, w, h)| w as usize * h as usize)
            .sum();
        assert_eq!(
            total,
            FB_W as usize * FB_H as usize,
            "tiles must cover the whole frame"
        );

        // Reconstruct a coverage map: each pixel hit exactly once (no gaps, no overlap).
        let mut covered = vec![0u8; FB_W as usize * FB_H as usize];
        for &(x, y, w, h) in &tiles {
            assert!(w <= 255 && h <= 255, "tile dims must fit u8");
            assert!(
                w as usize * h as usize <= 2048,
                "tile must fit one 0xA2 packet"
            );
            for dy in 0..h {
                for dx in 0..w {
                    covered[idx(x + dx, y + dy)] += 1;
                }
            }
        }
        assert!(
            covered.iter().all(|&c| c == 1),
            "every pixel covered exactly once"
        );
    }

    #[test]
    fn diff_tiles_change_outside_only_one_tile() {
        // Sanity: two distant single-pixel changes dirty exactly two distinct tiles.
        let prev = blank();
        let mut new = blank();
        new[idx(0, 0)] = 1; // tile (0,0)
        new[idx(319, 169)] = 1; // last tile

        let mut tiles = diff_tiles(&prev, &new, FB_W, FB_H, TILE_W, TILE_H);
        tiles.sort();
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0], (0, 0, 80, 16));
        assert_eq!(tiles[1], (240, 160, 80, 10));
    }

    // --- extract_tile ---

    #[test]
    fn extract_tile_pulls_correct_subrect() {
        let mut new = blank();
        // Fill the tile at (80,48) 80×16 with a known pattern keyed on position.
        for dy in 0..16u16 {
            for dx in 0..80u16 {
                new[idx(80 + dx, 48 + dy)] = dy * 80 + dx;
            }
        }
        let tile = extract_tile(&new, FB_W as usize, 80, 48, 80, 16);
        assert_eq!(tile.len(), 80 * 16);
        for (i, &px) in tile.iter().enumerate() {
            assert_eq!(px, i as u16, "row-major extraction mismatch at {}", i);
        }
    }

    // --- needs_full_redraw (the redraw_diff full-path decision) ---

    #[test]
    fn needs_full_redraw_force_flag() {
        assert!(
            needs_full_redraw(true, Some(54400), 54400),
            "force_full forces full"
        );
    }

    #[test]
    fn needs_full_redraw_no_prev() {
        assert!(
            needs_full_redraw(false, None, 54400),
            "first frame forces full"
        );
    }

    #[test]
    fn needs_full_redraw_length_mismatch() {
        // Orientation/size change: prev length differs → full redraw, prev reset.
        assert!(
            needs_full_redraw(false, Some(54400), 100),
            "length mismatch forces full"
        );
    }

    #[test]
    fn needs_full_redraw_steady_state_takes_diff_path() {
        assert!(
            !needs_full_redraw(false, Some(54400), 54400),
            "matching length without force takes the tile-diff path"
        );
    }
}
