#[macro_export]
macro_rules! time {
    ($name:expr, $x:block) => {{
        let start_time = chrono::Utc::now();

        let res = $x;

        let ms_elapsed = (chrono::Utc::now() - start_time).num_milliseconds();
        if ms_elapsed > 16 {
            tracing::warn!("{} took {} ms", $name, ms_elapsed);
        }

        res
    }};
}

#[macro_export]
macro_rules! time_us {
    ($name:expr, $thresh:literal, $x:block) => {{
        let start_time = chrono::Utc::now();

        let res = $x;

        let us_elapsed = (chrono::Utc::now() - start_time)
            .num_microseconds()
            .unwrap_or(i64::MAX);
        if us_elapsed > $thresh {
            tracing::info!("{} took {} us", $name, us_elapsed);
        }

        res
    }};
}
