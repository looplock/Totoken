use chrono::{DateTime, TimeZone, Utc};

pub fn now_utc() -> DateTime<Utc> {
    Utc::now()
}

pub fn format_utc_date(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%d").to_string()
}

pub fn from_unix_ms(value: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_millis_opt(value).single()
}
