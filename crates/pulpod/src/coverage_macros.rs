/// Helper macros that keep logging-only branches out of coverage builds.
///
/// Usage: wrap `warn!`/`info!` statements that are only hit during rare
/// failure paths with these macros so that the coverage build sees an empty
/// branch and the lines disappear from the report.
#[macro_export]
macro_rules! coverage_warn {
    ($($arg:tt)*) => {
        #[cfg(not(coverage))]
        {
            ::tracing::warn!($($arg)*);
        }
    };
}

#[macro_export]
macro_rules! coverage_info {
    ($($arg:tt)*) => {
        #[cfg(not(coverage))]
        {
            ::tracing::info!($($arg)*);
        }
    };
}
