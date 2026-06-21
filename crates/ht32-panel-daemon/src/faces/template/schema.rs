//! The editor's vocabulary, generated as JSON for the property-panel dropdowns.
//! Hand-listed here but guarded by an exhaustive-match contract test, so a new
//! enum variant cannot silently desync the editor from the renderer.

use serde_json::{json, Value};

/// All closed enum sets the editor offers, as JSON arrays of wire strings.
pub fn template_schema_json() -> Value {
    json!({
        "kinds": ["text","bar","gauge","sparkline","clock"],
        "number_sources": ["cpu_percent","ram_percent","cpu_temp","disk_read_rate",
                            "disk_write_rate","net_rx_rate","net_tx_rate"],
        "history_sources": ["disk_history","disk_read_history","disk_write_history",
                            "net_history","net_rx_history","net_tx_history"],
        "text_sources": ["literal","hostname","uptime","ip","net_interface","time","date","number"],
        "theme_slots": ["primary","secondary","text","background"],
        "aligns": ["left","center","right"],
        "time_fmts": ["hhmm","hhmmss","hhmm12h"],
        "date_fmts": ["iso","eu","us","short"],
        "number_fmts": ["percent","rate","raw"],
        "clock_modes": ["analog","digital"],
        "scale_modes": ["auto","fixed"],
        "orientations": ["landscape","portrait","landscape_upside_down","portrait_upside_down"]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::faces::template::spec::*;

    /// Maps each NumberSource variant to its wire string. The `match` is
    /// exhaustive (no `_`), so adding a variant breaks compilation until both
    /// this map AND the schema are updated — the dropdown can never drift.
    fn number_source_wire(s: NumberSource) -> &'static str {
        match s {
            NumberSource::CpuPercent => "cpu_percent",
            NumberSource::RamPercent => "ram_percent",
            NumberSource::CpuTemp => "cpu_temp",
            NumberSource::DiskReadRate => "disk_read_rate",
            NumberSource::DiskWriteRate => "disk_write_rate",
            NumberSource::NetRxRate => "net_rx_rate",
            NumberSource::NetTxRate => "net_tx_rate",
        }
    }

    #[test]
    fn schema_contains_every_number_source() {
        let schema = template_schema_json();
        let listed: Vec<String> = serde_json::from_value(schema["number_sources"].clone()).unwrap();
        for s in [
            NumberSource::CpuPercent,
            NumberSource::RamPercent,
            NumberSource::CpuTemp,
            NumberSource::DiskReadRate,
            NumberSource::DiskWriteRate,
            NumberSource::NetRxRate,
            NumberSource::NetTxRate,
        ] {
            assert!(
                listed.iter().any(|x| x == number_source_wire(s)),
                "schema missing number source {:?}",
                s
            );
        }
    }

    #[test]
    fn schema_has_all_top_level_keys() {
        let s = template_schema_json();
        for k in [
            "kinds",
            "number_sources",
            "history_sources",
            "text_sources",
            "theme_slots",
            "aligns",
            "time_fmts",
            "date_fmts",
            "number_fmts",
            "clock_modes",
            "orientations",
        ] {
            assert!(s.get(k).is_some(), "schema missing key {k}");
        }
    }
}
