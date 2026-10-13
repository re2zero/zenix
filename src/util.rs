//! Shared helpers that belong to no single layer.

/// Current UTC timestamp in ISO 8601 format (`YYYY-MM-DDTHH:MM:SS`),
/// matching what SQLite writes for `datetime()` columns.
pub fn timestamp_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            let days = secs / 86400;
            let time_of_day = secs % 86400;
            let h = time_of_day / 3600;
            let min = (time_of_day % 3600) / 60;
            let s = time_of_day % 60;
            // Simple date calculation from days since epoch
            // 1970-01-01 + days
            let mut y = 1970i32;
            let mut remaining_days = days as i32;
            loop {
                let diy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                    366
                } else {
                    365
                };
                if remaining_days < diy {
                    break;
                }
                remaining_days -= diy;
                y += 1;
            }
            let month_days = [
                31,
                if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                    29
                } else {
                    28
                },
                31,
                30,
                31,
                30,
                31,
                31,
                30,
                31,
                30,
                31,
            ];
            let mut m = 0usize;
            for (i, &md) in month_days.iter().enumerate() {
                if remaining_days < md {
                    m = i;
                    break;
                }
                remaining_days -= md;
            }
            let d = remaining_days + 1;
            format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}")
        })
        .unwrap_or_default()
}
