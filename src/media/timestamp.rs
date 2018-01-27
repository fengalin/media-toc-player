use std::fmt;

#[derive(Clone, Copy)]
pub struct Timestamp {
    pub nano_total: u64,
    pub nano: u64,
    pub us: u64,
    pub ms: u64,
    pub s: u64,
    pub m: u64,
    pub h: u64,
}

impl Timestamp {
    pub fn from_nano(nano_total: u64) -> Self {
        let us_total = nano_total / 1_000;
        let ms_total = us_total / 1_000;
        let s_total = ms_total / 1_000;
        let m_total = s_total / 60;

        Timestamp {
            nano_total: nano_total,
            nano: nano_total % 1_000,
            us: us_total % 1_000,
            ms: ms_total % 1_000,
            s: s_total % 60,
            m: m_total % 60,
            h: m_total / 60,
        }
    }

    pub fn from_signed_nano(nano: i64) -> Self {
        if nano.is_negative() {
            Timestamp {
                nano_total: 0,
                nano: 0,
                us: 0,
                ms: 0,
                s: 0,
                m: 0,
                h: 0,
            }
        } else {
            Timestamp::from_nano(nano as u64)
        }
    }

    pub fn format(nano_total: u64, with_micro: bool) -> String {
        let us_total = nano_total / 1_000;
        let ms_total = us_total / 1_000;
        let s_total = ms_total / 1_000;
        let m_total = s_total / 60;
        let h = m_total / 60;

        let micro = if with_micro {
            format!(".{:03}", us_total % 1_000)
        } else {
            "".to_owned()
        };
        if h == 0 {
            format!(
                "{:02}:{:02}.{:03}{}",
                m_total % 60,
                s_total % 60,
                ms_total % 1_000,
                micro
            ).to_owned()
        } else {
            format!(
                "{:02}:{:02}:{:02}.{:03}{}",
                h,
                m_total % 60,
                s_total % 60,
                ms_total % 1_000,
                micro
            ).to_owned()
        }
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let prefix = if self.h > 0 {
            format!("{:02}:", self.h).to_owned()
        } else {
            String::new()
        };

        write!(f, "{}{:02}:{:02}.{:03}", prefix, self.m, self.s, self.ms)
    }
}

#[cfg(test)]
mod tests {
    use metadata::Timestamp;

    #[test]
    fn parse_string() {
        let ts = Timestamp::from_string("10:42:20.010");
        assert_eq!(ts.h, 10);
        assert_eq!(ts.m, 42);
        assert_eq!(ts.s, 20);
        assert_eq!(ts.ms, 10);
        assert_eq!(ts.us, 0);
        assert_eq!(ts.nano, 0);
        assert_eq!(
            ts.nano_total,
            ((((10 * 60 + 42) * 60 + 20) * 1_000) + 10) * 1_000 * 1_000
        );

        let ts = Timestamp::from_string("42:20.010");
        assert_eq!(ts.h, 0);
        assert_eq!(ts.m, 42);
        assert_eq!(ts.s, 20);
        assert_eq!(ts.ms, 10);
        assert_eq!(ts.us, 0);
        assert_eq!(ts.nano, 0);
        assert_eq!(
            ts.nano_total,
            (((42 * 60 + 20) * 1_000) + 10) * 1_000 * 1_000
        );

        let ts = Timestamp::from_string("42:20.010.015");
        assert_eq!(ts.h, 0);
        assert_eq!(ts.m, 42);
        assert_eq!(ts.s, 20);
        assert_eq!(ts.ms, 10);
        assert_eq!(ts.us, 15);
        assert_eq!(ts.nano, 0);
        assert_eq!(
            ts.nano_total,
            ((((42 * 60 + 20) * 1_000) + 10) * 1_000 + 15) * 1_000
        );
    }
}
