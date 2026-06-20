//! System data aggregation for faces.

use std::collections::VecDeque;

/// Number of history samples to keep for graphs.
pub const HISTORY_SIZE: usize = 60;

/// IP address display preference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IpDisplayPreference {
    /// IPv6 Global Unicast Address (2000::/3)
    #[default]
    Ipv6Gua,
    /// IPv6 Link-Local Address (fe80::/10)
    Ipv6Lla,
    /// IPv6 Unique Local Address (fc00::/7)
    Ipv6Ula,
    /// IPv4 address
    Ipv4,
}

impl IpDisplayPreference {
    /// Returns all available preferences.
    pub fn all() -> &'static [IpDisplayPreference] {
        &[
            IpDisplayPreference::Ipv6Gua,
            IpDisplayPreference::Ipv6Lla,
            IpDisplayPreference::Ipv6Ula,
            IpDisplayPreference::Ipv4,
        ]
    }

    /// Returns the display name for this preference.
    pub fn display_name(&self) -> &'static str {
        match self {
            IpDisplayPreference::Ipv6Gua => "IPv6 GUA",
            IpDisplayPreference::Ipv6Lla => "IPv6 LLA",
            IpDisplayPreference::Ipv6Ula => "IPv6 ULA",
            IpDisplayPreference::Ipv4 => "IPv4",
        }
    }
}

impl std::fmt::Display for IpDisplayPreference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpDisplayPreference::Ipv6Gua => write!(f, "ipv6-gua"),
            IpDisplayPreference::Ipv6Lla => write!(f, "ipv6-lla"),
            IpDisplayPreference::Ipv6Ula => write!(f, "ipv6-ula"),
            IpDisplayPreference::Ipv4 => write!(f, "ipv4"),
        }
    }
}

impl std::str::FromStr for IpDisplayPreference {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ipv6-gua" | "ipv6_gua" | "ipv6gua" | "gua" => Ok(IpDisplayPreference::Ipv6Gua),
            "ipv6-lla" | "ipv6_lla" | "ipv6lla" | "lla" => Ok(IpDisplayPreference::Ipv6Lla),
            "ipv6-ula" | "ipv6_ula" | "ipv6ula" | "ula" => Ok(IpDisplayPreference::Ipv6Ula),
            "ipv4" | "v4" => Ok(IpDisplayPreference::Ipv4),
            _ => Err(format!("Unknown IP display preference: {}", s)),
        }
    }
}

/// Aggregated system data from all sensors.
#[derive(Debug, Clone, Default)]
pub struct SystemData {
    /// Hostname of the system
    pub hostname: String,
    /// Current time formatted as "HH:MM" (24h format, for backwards compatibility)
    pub time: String,
    /// Hour (0-23)
    pub hour: u8,
    /// Minute (0-59)
    pub minute: u8,
    /// Day of month (1-31)
    pub day: u8,
    /// Month (1-12)
    pub month: u8,
    /// Year (e.g., 2024)
    pub year: u16,
    /// Day of week (0=Sunday, 1=Monday, ..., 6=Saturday)
    pub day_of_week: u8,
    /// Uptime formatted as "Xd Yh Zm"
    pub uptime: String,
    /// CPU usage percentage (0-100)
    pub cpu_percent: f64,
    /// CPU temperature in Celsius (None if unavailable)
    pub cpu_temp: Option<f64>,
    /// RAM usage percentage (0-100)
    pub ram_percent: f64,
    /// Disk read rate in bytes/second
    pub disk_read_rate: f64,
    /// Disk write rate in bytes/second
    pub disk_write_rate: f64,
    /// Disk I/O history (combined read+write rates, newest last)
    pub disk_history: VecDeque<f64>,
    /// Disk read history (bytes/sec, newest last)
    pub disk_read_history: VecDeque<f64>,
    /// Disk write history (bytes/sec, newest last)
    pub disk_write_history: VecDeque<f64>,
    /// Network interface name
    pub net_interface: String,
    /// Network receive rate in bytes/second
    pub net_rx_rate: f64,
    /// Network transmit rate in bytes/second
    pub net_tx_rate: f64,
    /// Network I/O history (combined rx+tx rates, newest last)
    pub net_history: VecDeque<f64>,
    /// Network receive history (bytes/sec, newest last)
    pub net_rx_history: VecDeque<f64>,
    /// Network transmit history (bytes/sec, newest last)
    pub net_tx_history: VecDeque<f64>,
    /// IP address to display (based on preference)
    pub display_ip: Option<String>,
    /// Monotonic count of disk combined-history samples ever pushed (for wrap-around graphs).
    pub disk_sample_count: u64,
    /// Monotonic count of network combined-history samples ever pushed (for wrap-around graphs).
    pub net_sample_count: u64,
}

impl SystemData {
    /// Formats time according to the specified format.
    pub fn format_time(&self, format: &str) -> String {
        match format {
            "digital-12h" => {
                let (hour_12, am_pm) = if self.hour == 0 {
                    (12, "AM")
                } else if self.hour < 12 {
                    (self.hour, "AM")
                } else if self.hour == 12 {
                    (12, "PM")
                } else {
                    (self.hour - 12, "PM")
                };
                format!("{:2}:{:02} {}", hour_12, self.minute, am_pm)
            }
            "analogue" => {
                // For analogue, return empty - faces should draw a clock
                String::new()
            }
            // Default to digital-24h
            _ => format!("{:02}:{:02}", self.hour, self.minute),
        }
    }

    /// Formats date according to the specified format.
    pub fn format_date(&self, format: &str) -> Option<String> {
        let month_names = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        let month_abbrev = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let weekday_abbrev = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

        let month_idx = (self.month.saturating_sub(1) as usize).min(11);
        let weekday_idx = (self.day_of_week as usize).min(6);

        match format {
            "hidden" => None,
            "iso" => Some(format!(
                "{:04}-{:02}-{:02}",
                self.year, self.month, self.day
            )),
            "us" => Some(format!(
                "{:02}/{:02}/{:04}",
                self.month, self.day, self.year
            )),
            "eu" => Some(format!(
                "{:02}/{:02}/{:04}",
                self.day, self.month, self.year
            )),
            "short" => Some(format!("{} {}", month_abbrev[month_idx], self.day)),
            "long" => Some(format!(
                "{} {}, {}",
                month_names[month_idx], self.day, self.year
            )),
            "weekday" => Some(format!(
                "{}, {} {}",
                weekday_abbrev[weekday_idx], month_abbrev[month_idx], self.day
            )),
            _ => None,
        }
    }

    /// Formats a byte rate as a human-readable string (e.g., "1.2 MB/s")
    pub fn format_rate(bytes_per_sec: f64) -> String {
        if bytes_per_sec >= 1_000_000_000.0 {
            format!("{:.1} GB/s", bytes_per_sec / 1_000_000_000.0)
        } else if bytes_per_sec >= 1_000_000.0 {
            format!("{:.1} MB/s", bytes_per_sec / 1_000_000.0)
        } else if bytes_per_sec >= 1_000.0 {
            format!("{:.1} KB/s", bytes_per_sec / 1_000.0)
        } else {
            format!("{:.0} B/s", bytes_per_sec)
        }
    }

    /// Formats a byte rate compactly (e.g., "1.2M")
    pub fn format_rate_compact(bytes_per_sec: f64) -> String {
        if bytes_per_sec >= 1_000_000_000.0 {
            format!("{:.1}G", bytes_per_sec / 1_000_000_000.0)
        } else if bytes_per_sec >= 1_000_000.0 {
            format!("{:.1}M", bytes_per_sec / 1_000_000.0)
        } else if bytes_per_sec >= 1_000.0 {
            format!("{:.1}K", bytes_per_sec / 1_000.0)
        } else {
            format!("{:.0}B", bytes_per_sec)
        }
    }

    /// Computes an appropriate max scale value for graphing I/O history.
    ///
    /// This provides auto-scaling so graphs remain useful at any rate.
    /// The returned value is rounded up to a "nice" number to avoid
    /// constant scale changes.
    pub fn compute_graph_scale(history: &VecDeque<f64>) -> f64 {
        const MIN_SCALE: f64 = 1_000_000.0; // 1 MB/s minimum

        let max_val = history.iter().copied().fold(0.0_f64, |a, b| a.max(b));

        if max_val <= MIN_SCALE {
            return MIN_SCALE;
        }

        // Round up to next power of 10, then snap to 1x, 2x, or 5x
        let magnitude = 10_f64.powf(max_val.log10().floor());
        let normalized = max_val / magnitude;

        let multiplier = if normalized <= 1.0 {
            1.0
        } else if normalized <= 2.0 {
            2.0
        } else if normalized <= 5.0 {
            5.0
        } else {
            10.0
        };

        (magnitude * multiplier).max(MIN_SCALE)
    }
}
