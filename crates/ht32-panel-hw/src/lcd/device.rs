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

    /// Performs a full screen redraw.
    pub fn redraw(&self, framebuffer: &Framebuffer) -> Result<()> {
        let orientation = *self.current_orientation.lock().unwrap();
        let mut data = framebuffer.data().to_vec();

        // Apply software rotation if needed
        if orientation.needs_rotation() {
            Orientation::rotate_180(&mut data, framebuffer.width(), framebuffer.height());
        }

        let device = self.device.lock().unwrap();

        for chunk_idx in 0..CHUNK_COUNT {
            let offset = chunk_idx * (DATA_SIZE / 2);
            let packet = build_redraw_chunk(chunk_idx, &data, offset);

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
}
