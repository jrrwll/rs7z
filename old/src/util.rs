use chrono::{DateTime, Local};

pub fn format_size(size: u64) -> String {
    if size == 0 {
        String::new()
    } else if size < 1024 {
        format!("{} B", size)
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

pub fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return String::new();
    }
    if let Some(dt) = DateTime::from_timestamp(ts as i64, 0) {
        let dt = dt.with_timezone(&Local);
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        String::from("invalid_date")
    }
}
