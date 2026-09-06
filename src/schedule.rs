//! 集計対象の期間を計算する。
//!
//! 定期投稿は日本時間の暦（月曜始まりの週、月初）を基準にする。
//! ここも外部に依存しない計算だけなのでテストできる。

use chrono::{DateTime, Datelike, Days, FixedOffset, Months, NaiveDate, TimeZone};

/// 日本時間（UTC+9）
const JST_OFFSET: i32 = 9 * 3600;

/// 定期投稿を行う時刻（日本時間）
const POST_HOUR: i64 = 9;

const DAY: i64 = 86_400;

/// 集計する期間。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// 開始（UNIX秒、この時刻を含む）。0 は全期間を意味する。
    pub start: i64,
    /// 終了（UNIX秒、この時刻を含まない）
    pub end: i64,
    /// 前期比を出すための、ひとつ前の期間の開始
    pub previous_start: Option<i64>,
    /// 見出しに出す期間の名前
    pub label: String,
    /// 前期比の呼び方。比較しない場合は `None`
    pub comparison: Option<&'static str>,
    /// 定期投稿の重複を判定するための識別子。手動実行では `None`
    pub key: Option<String>,
}

impl Span {
    /// この期間に含まれる時刻かどうか。
    pub fn contains(&self, timestamp: i64) -> bool {
        timestamp >= self.start && timestamp < self.end
    }
}

fn jst() -> FixedOffset {
    FixedOffset::east_opt(JST_OFFSET).expect("固定オフセットが不正")
}

fn to_jst(timestamp: i64) -> DateTime<FixedOffset> {
    DateTime::from_timestamp(timestamp, 0)
        .unwrap_or_default()
        .with_timezone(&jst())
}

/// 日本時間における、その日の0時を UNIX 秒で返す。
fn start_of_day(date: NaiveDate) -> i64 {
    let naive = date.and_hms_opt(0, 0, 0).expect("0時は常に有効");
    jst()
        .from_local_datetime(&naive)
        .single()
        .expect("固定オフセットなら常に一意に定まる")
        .timestamp()
}

/// 「現在からさかのぼって N 日間」の期間。手動実行用。
pub fn rolling(now: i64, days: i64, label: &str, comparison: &'static str) -> Span {
    let start = now - days * DAY;
    Span {
        start,
        end: now,
        previous_start: Some(start - days * DAY),
        label: label.to_string(),
        comparison: Some(comparison),
        key: None,
    }
}

/// 全期間。
pub fn all_time(now: i64) -> Span {
    Span {
        start: 0,
        end: now,
        previous_start: None,
        label: "全期間".to_string(),
        comparison: None,
        key: None,
    }
}

/// いま投稿されるべき週次ランキングの期間（先週の月曜〜日曜）。
///
/// 月曜9時を過ぎていればその週の分、まだであればひとつ前の週の分を返す。
pub fn due_weekly(now: i64) -> Span {
    let today = to_jst(now).date_naive();
    let this_monday = today - Days::new(today.weekday().num_days_from_monday() as u64);

    // 今週の月曜9時をまだ過ぎていなければ、対象はひとつ前の週
    let post_monday = if now >= start_of_day(this_monday) + POST_HOUR * 3600 {
        this_monday
    } else {
        this_monday - Days::new(7)
    };

    let start_date = post_monday - Days::new(7);
    let last_date = post_monday - Days::new(1);
    let iso = start_date.iso_week();

    Span {
        start: start_of_day(start_date),
        end: start_of_day(post_monday),
        previous_start: Some(start_of_day(start_date - Days::new(7))),
        label: format!(
            "{}月{}日〜{}月{}日",
            start_date.month(),
            start_date.day(),
            last_date.month(),
            last_date.day()
        ),
        comparison: Some("前週比"),
        key: Some(format!("週次 {}-W{:02}", iso.year(), iso.week())),
    }
}

/// いま投稿されるべき月次ランキングの期間（先月まるごと）。
pub fn due_monthly(now: i64) -> Span {
    let this_first = to_jst(now)
        .date_naive()
        .with_day(1)
        .expect("1日は常に有効");

    let post_first = if now >= start_of_day(this_first) + POST_HOUR * 3600 {
        this_first
    } else {
        this_first - Months::new(1)
    };

    let start_date = post_first - Months::new(1);

    Span {
        start: start_of_day(start_date),
        end: start_of_day(post_first),
        previous_start: Some(start_of_day(start_date - Months::new(1))),
        label: format!("{}年{}月", start_date.year(), start_date.month()),
        comparison: Some("前月比"),
        key: Some(format!(
            "月次 {}-{:02}",
            start_date.year(),
            start_date.month()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 日本時間の日時を UNIX 秒にする（テスト用）
    fn jst_at(y: i32, m: u32, d: u32, hour: u32) -> i64 {
        let naive = NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(hour, 0, 0)
            .unwrap();
        jst().from_local_datetime(&naive).single().unwrap().timestamp()
    }

    #[test]
    fn 週次は先週の月曜から日曜までを対象にする() {
        // 2026年9月7日は月曜日。その9時ちょうど。
        let span = due_weekly(jst_at(2026, 9, 7, 9));

        assert_eq!(span.start, jst_at(2026, 8, 31, 0), "先週の月曜0時から");
        assert_eq!(span.end, jst_at(2026, 9, 7, 0), "今週の月曜0時まで");
        assert_eq!(span.label, "8月31日〜9月6日");
        assert_eq!(span.comparison, Some("前週比"));
    }

    #[test]
    fn 月曜9時より前ならひとつ前の週が対象() {
        // 同じ月曜の8時59分。まだ投稿時刻を過ぎていない。
        let before = due_weekly(jst_at(2026, 9, 7, 8));
        let after = due_weekly(jst_at(2026, 9, 7, 9));

        assert_eq!(before.end, jst_at(2026, 8, 31, 0));
        assert_ne!(before.key, after.key, "別の週として扱われる");
    }

    #[test]
    fn 週の途中ではその週の分を対象にし続ける() {
        // 火曜・日曜のどこで実行しても、直前の月曜に確定した期間は変わらない
        let tuesday = due_weekly(jst_at(2026, 9, 8, 15));
        let sunday = due_weekly(jst_at(2026, 9, 13, 23));

        assert_eq!(tuesday, sunday, "同じ週の中では対象がぶれない");
        assert_eq!(tuesday.start, jst_at(2026, 8, 31, 0));
    }

    #[test]
    fn 週次の識別子は週ごとに変わる() {
        let a = due_weekly(jst_at(2026, 9, 7, 9));
        let b = due_weekly(jst_at(2026, 9, 14, 9));

        assert!(a.key.is_some());
        assert_ne!(a.key, b.key);
    }

    #[test]
    fn 月次は先月まるごとを対象にする() {
        let span = due_monthly(jst_at(2026, 9, 1, 9));

        assert_eq!(span.start, jst_at(2026, 8, 1, 0));
        assert_eq!(span.end, jst_at(2026, 9, 1, 0));
        assert_eq!(span.label, "2026年8月");
        assert_eq!(span.key, Some("月次 2026-08".to_string()));
        assert_eq!(span.previous_start, Some(jst_at(2026, 7, 1, 0)));
    }

    #[test]
    fn 月初9時より前ならひとつ前の月が対象() {
        let span = due_monthly(jst_at(2026, 9, 1, 8));

        assert_eq!(span.label, "2026年7月");
        assert_eq!(span.end, jst_at(2026, 8, 1, 0));
    }

    #[test]
    fn 年をまたいでも正しく計算できる() {
        let span = due_monthly(jst_at(2026, 1, 1, 9));

        assert_eq!(span.label, "2025年12月");
        assert_eq!(span.start, jst_at(2025, 12, 1, 0));
        assert_eq!(span.key, Some("月次 2025-12".to_string()));
    }

    #[test]
    fn 手動実行の期間には識別子がない() {
        // 識別子がないことで、定期投稿の重複判定に混ざらない
        let now = jst_at(2026, 9, 7, 12);
        assert!(rolling(now, 7, "直近7日間", "前週比").key.is_none());
        assert!(all_time(now).key.is_none());
    }

    #[test]
    fn 直近n日間の期間を計算できる() {
        let now = jst_at(2026, 9, 7, 12);
        let span = rolling(now, 7, "直近7日間", "前週比");

        assert_eq!(span.end, now);
        assert_eq!(span.start, now - 7 * DAY);
        assert_eq!(span.previous_start, Some(now - 14 * DAY));
    }

    #[test]
    fn 全期間には開始も比較もない() {
        let now = jst_at(2026, 9, 7, 12);
        let span = all_time(now);

        assert_eq!(span.start, 0);
        assert_eq!(span.previous_start, None);
        assert_eq!(span.comparison, None);
    }

    #[test]
    fn 期間の内外を判定できる() {
        let span = due_weekly(jst_at(2026, 9, 7, 9));

        assert!(span.contains(jst_at(2026, 9, 1, 12)));
        assert!(span.contains(jst_at(2026, 8, 31, 0)), "開始時刻は含む");
        assert!(!span.contains(jst_at(2026, 9, 7, 0)), "終了時刻は含まない");
        assert!(!span.contains(jst_at(2026, 8, 30, 23)));
    }
}
