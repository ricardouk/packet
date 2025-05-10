use std::{
    collections::VecDeque,
    fmt,
    time::{self},
};

use gettextrs::ngettext;

#[macro_export]
macro_rules! impl_deref_for_newtype {
    ($type:ty, $target:ty) => {
        impl std::ops::Deref for $type {
            type Target = $target;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl std::ops::DerefMut for $type {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
    };
}

const STEPS_TRACK_COUNT: usize = 5;

/// Proudly stolen from:\
/// https://github.com/Manishearth/rustup.rs/blob/1.0.0/src/rustup-cli/download_tracker.rs
#[derive(Debug, Clone, better_default::Default)]
pub struct DataTransferEta {
    total_len: usize,
    total_transferred: usize,

    transferred_this_sec: usize,

    #[default(VecDeque::with_capacity(STEPS_TRACK_COUNT))]
    transferred_last_few_secs: VecDeque<usize>,

    last_sec: Option<time::Instant>,
    seconds_elapsed: usize,
}

impl DataTransferEta {
    pub fn new(len: usize) -> Self {
        Self {
            total_len: len,
            ..Default::default()
        }
    }

    pub fn step_with(&mut self, total_transferred: usize) {
        let len = total_transferred - self.total_transferred;
        self.transferred_this_sec += len;
        self.total_transferred = total_transferred;

        let current_time = time::Instant::now();

        match self.last_sec {
            None => {
                self.last_sec = Some(current_time);
            }
            Some(start) => {
                let elapsed = current_time - start;

                if elapsed.as_secs_f64() >= 1.0 {
                    self.seconds_elapsed += 1;

                    self.last_sec = Some(current_time);
                    if self.transferred_last_few_secs.len() == STEPS_TRACK_COUNT {
                        self.transferred_last_few_secs.pop_back();
                    }
                    self.transferred_last_few_secs
                        .push_front(self.transferred_this_sec);
                    self.transferred_this_sec = 0;
                }
            }
        };
    }

    pub fn prepare_for_new_transfer(&mut self, total_len: Option<usize>) {
        if let Some(total_len) = total_len {
            self.total_len = total_len;
        }
        self.total_transferred = 0;
        self.transferred_this_sec = 0;
        self.transferred_last_few_secs.clear();
        self.seconds_elapsed = 0;
        self.last_sec = None;
    }

    pub fn get_estimate_string(&self) -> String {
        let sum = self
            .transferred_last_few_secs
            .iter()
            .fold(0., |a, &v| a + v as f64);
        let len = self.transferred_last_few_secs.len();
        let speed = if len > 0 { sum / len as f64 } else { 0. };

        let total_len = self.total_len as f64;
        let remaining = total_len - self.total_transferred as f64;
        let eta_h = HumanReadable(remaining / speed);

        eta_h.to_string()
    }
}

#[derive(Debug, Clone, Copy)]
struct HumanReadable(f64);

impl fmt::Display for HumanReadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sec = self.0;

        if sec.is_infinite() {
            write!(f, "Unknown")
        } else {
            // we're doing modular arithmetic, treat as integer
            let sec = self.0 as u32;
            if sec > 6_000 {
                let h = sec / 3600;
                let min = sec % 3600;

                write!(
                    f,
                    "{:3} {} {:2} {}",
                    h,
                    ngettext("hour", "hours", h),
                    min,
                    ngettext("minute", "minutes", min)
                )
            } else if sec > 100 {
                let min = sec / 60;
                let sec = sec % 60;

                write!(
                    f,
                    "{:3} {} {:2} {}",
                    min,
                    ngettext("minute", "minutes", min),
                    sec,
                    ngettext("second", "seconds", min)
                )
            } else {
                write!(f, "{:3.0} {}", self.0, ngettext("second", "seconds", sec))
            }
        }
    }
}
