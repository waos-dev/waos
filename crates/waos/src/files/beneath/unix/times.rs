use rustix::fs::{UTIME_NOW, UTIME_OMIT};
use rustix::time::Timespec;
use std::io;
use std::num::TryFromIntError;
use std::time::SystemTime;

pub(crate) fn to_timespec(ft: Option<SystemTime>) -> io::Result<Timespec> {
    Ok(match ft {
        None => Timespec {
            tv_sec: 0,
            tv_nsec: UTIME_OMIT.into(),
        },
        Some(ft) => {
            let date_time = Datetime::try_from(ft).map_err(|e| io::Error::other(e))?;
            let Ok(nanoseconds) = date_time.nanoseconds.try_into();
            assert_ne!(i64::from(nanoseconds), i64::from(UTIME_OMIT));
            assert_ne!(i64::from(nanoseconds), i64::from(UTIME_NOW));
            Timespec {
                tv_sec: date_time.seconds,
                tv_nsec: nanoseconds,
            }
        }
    })
}

/// A time as whole seconds from the epoch, before it when negative, and the nanoseconds after
/// them, as `wasmtime-wasi`'s `clocks::Datetime` makes one from a `SystemTime`.
struct Datetime {
    seconds: i64,
    nanoseconds: u32,
}

impl TryFrom<SystemTime> for Datetime {
    type Error = TryFromIntError;

    fn try_from(time: SystemTime) -> Result<Self, Self::Error> {
        match time.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(duration) => Ok(Self {
                seconds: duration.as_secs().try_into()?,
                nanoseconds: duration.subsec_nanos(),
            }),
            Err(e) => {
                // If time is before the epoch, the error gives us the duration
                // from `time` to the epoch.
                let duration = e.duration();
                // So both seconds and nanoseconds are negative. We need to
                // convert it to negative seconds and positive nanos.
                let nanoseconds = duration.subsec_nanos();
                if nanoseconds == 0 {
                    Ok(Self {
                        seconds: (-i128::from(duration.as_secs())).try_into()?,
                        nanoseconds: 0,
                    })
                } else {
                    Ok(Self {
                        seconds: (-i128::from(duration.as_secs()) - 1).try_into()?,
                        nanoseconds: 1_000_000_000 - nanoseconds,
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn before_unix_epoch() {
        let epoch = SystemTime::UNIX_EPOCH;
        let cases = [
            (
                epoch.checked_sub(Duration::from_secs(1)),
                Timespec {
                    tv_sec: -1,
                    tv_nsec: 0,
                },
            ),
            (
                epoch.checked_sub(Duration::from_millis(500)),
                Timespec {
                    tv_sec: -1,
                    tv_nsec: 500_000_000,
                },
            ),
            (
                epoch.checked_sub(Duration::from_nanos(1)),
                Timespec {
                    tv_sec: -1,
                    tv_nsec: 999_999_999,
                },
            ),
            (
                epoch.checked_add(Duration::from_millis(500)),
                Timespec {
                    tv_sec: 0,
                    tv_nsec: 500_000_000,
                },
            ),
        ];

        for (time, expected) in cases {
            assert_eq!(to_timespec(time).unwrap(), expected);
        }
    }
}
