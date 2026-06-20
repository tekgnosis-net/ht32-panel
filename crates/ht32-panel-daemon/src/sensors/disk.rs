//! Disk I/O sensor.

use super::data::HISTORY_SIZE;
use super::Sensor;
use std::collections::VecDeque;
use std::fs;
use std::time::Instant;

/// Disk I/O sensor that reads from /proc/diskstats.
pub struct DiskSensor {
    name: String,
    device: String,
    last_read_sectors: u64,
    last_write_sectors: u64,
    last_time: Option<Instant>,
    last_read_rate: f64,
    last_write_rate: f64,
    /// History of combined I/O rates (bytes/sec)
    history: VecDeque<f64>,
    /// History of read rates (bytes/sec)
    read_history: VecDeque<f64>,
    /// History of write rates (bytes/sec)
    write_history: VecDeque<f64>,
    /// Monotonic count of combined-history samples ever pushed.
    samples_pushed: u64,
}

impl DiskSensor {
    /// Creates a new disk sensor for a specific device (e.g., "sda", "nvme0n1").
    pub fn new(device: &str) -> Self {
        Self {
            name: format!("disk_{}", device),
            device: device.to_string(),
            last_read_sectors: 0,
            last_write_sectors: 0,
            last_time: None,
            last_read_rate: 0.0,
            last_write_rate: 0.0,
            history: VecDeque::with_capacity(HISTORY_SIZE),
            read_history: VecDeque::with_capacity(HISTORY_SIZE),
            write_history: VecDeque::with_capacity(HISTORY_SIZE),
            samples_pushed: 0,
        }
    }

    /// Creates a disk sensor that auto-detects the primary disk.
    pub fn auto() -> Self {
        // Try to find the primary disk
        let device = Self::detect_primary_disk().unwrap_or_else(|| "sda".to_string());
        Self::new(&device)
    }

    /// Detects the primary disk device.
    fn detect_primary_disk() -> Option<String> {
        // Try common disk device names in order of preference
        let candidates = ["nvme0n1", "sda", "vda", "xvda", "mmcblk0"];

        for candidate in candidates {
            let path = format!("/sys/block/{}", candidate);
            if std::path::Path::new(&path).exists() {
                return Some(candidate.to_string());
            }
        }

        None
    }

    /// Reads disk stats from /proc/diskstats.
    ///
    /// Format: https://www.kernel.org/doc/Documentation/ABI/testing/procfs-diskstats
    /// Fields: major minor name reads_completed reads_merged sectors_read time_reading
    ///         writes_completed writes_merged sectors_written time_writing
    ///         ios_in_progress time_doing_io weighted_time_doing_io
    fn read_stats(&self) -> Option<(u64, u64)> {
        let content = fs::read_to_string("/proc/diskstats").ok()?;

        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 10 && parts[2] == self.device {
                // sectors_read is field 5 (0-indexed)
                // sectors_written is field 9 (0-indexed)
                let read_sectors: u64 = parts[5].parse().ok()?;
                let write_sectors: u64 = parts[9].parse().ok()?;
                return Some((read_sectors, write_sectors));
            }
        }

        None
    }

    /// Returns the current read rate in bytes/second.
    pub fn read_rate(&self) -> f64 {
        self.last_read_rate
    }

    /// Returns the current write rate in bytes/second.
    pub fn write_rate(&self) -> f64 {
        self.last_write_rate
    }

    /// Returns the I/O history (combined read+write rates).
    pub fn history(&self) -> &VecDeque<f64> {
        &self.history
    }

    /// Returns the read rate history (bytes/sec).
    pub fn read_history(&self) -> &VecDeque<f64> {
        &self.read_history
    }

    /// Returns the write rate history (bytes/sec).
    pub fn write_history(&self) -> &VecDeque<f64> {
        &self.write_history
    }

    /// Returns the total number of combined-history samples ever pushed.
    ///
    /// This monotonic counter lets wrap-around graph renderers place sample `i`
    /// at column `i % HISTORY_SIZE` without ambiguity after the ring wraps.
    pub fn sample_count(&self) -> u64 {
        self.samples_pushed
    }

    /// Appends one tick's measurements to all three history rings and increments
    /// the monotonic sample counter.
    ///
    /// Extracted from `sample()` so unit tests can drive history without needing
    /// `/proc/diskstats`.
    fn record_sample(&mut self, combined: f64, read: f64, write: f64) {
        if self.history.len() >= HISTORY_SIZE {
            self.history.pop_front();
        }
        self.history.push_back(combined);
        self.samples_pushed += 1;

        if self.read_history.len() >= HISTORY_SIZE {
            self.read_history.pop_front();
        }
        self.read_history.push_back(read);

        if self.write_history.len() >= HISTORY_SIZE {
            self.write_history.pop_front();
        }
        self.write_history.push_back(write);
    }
}

impl Sensor for DiskSensor {
    fn name(&self) -> &str {
        &self.name
    }

    fn sample(&mut self) -> f64 {
        if let Some((read_sectors, write_sectors)) = self.read_stats() {
            if let Some(last_time) = self.last_time {
                let elapsed = last_time.elapsed().as_secs_f64();
                if elapsed > 0.0 {
                    let read_delta = read_sectors.saturating_sub(self.last_read_sectors);
                    let write_delta = write_sectors.saturating_sub(self.last_write_sectors);

                    // Sectors are typically 512 bytes
                    const SECTOR_SIZE: f64 = 512.0;
                    self.last_read_rate = (read_delta as f64 * SECTOR_SIZE) / elapsed;
                    self.last_write_rate = (write_delta as f64 * SECTOR_SIZE) / elapsed;

                    // Record combined and separate histories (increments samples_pushed).
                    let combined = self.last_read_rate + self.last_write_rate;
                    self.record_sample(combined, self.last_read_rate, self.last_write_rate);
                }
            }

            self.last_read_sectors = read_sectors;
            self.last_write_sectors = write_sectors;
            self.last_time = Some(Instant::now());
        }

        // Return combined rate in KB/s
        (self.last_read_rate + self.last_write_rate) / 1024.0
    }

    fn min(&self) -> f64 {
        0.0
    }

    fn max(&self) -> f64 {
        1000000.0 // 1 GB/s max
    }

    fn unit(&self) -> &str {
        "KB/s"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_sensor_counts_samples() {
        let mut s = DiskSensor::new("sda");
        let before = s.sample_count();
        s.record_sample(1.0, 0.5, 0.5);
        s.record_sample(2.0, 1.0, 1.0);
        assert_eq!(s.sample_count(), before + 2);
    }

    #[test]
    fn disk_sensor_sample_count_starts_at_zero() {
        let s = DiskSensor::new("sda");
        assert_eq!(s.sample_count(), 0);
    }
}
