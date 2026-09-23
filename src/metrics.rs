use rusqlite::{Connection, params};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Counter {
    MessagesSeen,
    AiReviews,
    AiCompliant,
    AiGrayArea,
    AiViolations,
    AiErrors,
    AiDeletedMessages,
    AiDeleteFailures,
    StaffReviewPings,
    BotReplies,
    RuleSourceMessages,
    RulesImported,
    CommandsUsed,
    UploadsAdded,
    UploadsRemoved,
}

impl Counter {
    fn column(self) -> &'static str {
        match self {
            Self::MessagesSeen => "messages_seen",
            Self::AiReviews => "ai_reviews",
            Self::AiCompliant => "ai_compliant",
            Self::AiGrayArea => "ai_gray_area",
            Self::AiViolations => "ai_violations",
            Self::AiErrors => "ai_errors",
            Self::AiDeletedMessages => "ai_deleted_messages",
            Self::AiDeleteFailures => "ai_delete_failures",
            Self::StaffReviewPings => "staff_review_pings",
            Self::BotReplies => "bot_replies",
            Self::RuleSourceMessages => "rule_source_messages",
            Self::RulesImported => "rules_imported",
            Self::CommandsUsed => "commands_used",
            Self::UploadsAdded => "uploads_added",
            Self::UploadsRemoved => "uploads_removed",
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub(crate) struct Totals {
    pub messages_seen: u64,
    pub ai_reviews: u64,
    pub ai_compliant: u64,
    pub ai_gray_area: u64,
    pub ai_violations: u64,
    pub ai_errors: u64,
    pub ai_deleted_messages: u64,
    pub ai_delete_failures: u64,
    pub staff_review_pings: u64,
    pub bot_replies: u64,
    pub rule_source_messages: u64,
    pub rules_imported: u64,
    pub commands_used: u64,
    pub uploads_added: u64,
    pub uploads_removed: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

pub(crate) fn schema() -> &'static str {
    "CREATE TABLE IF NOT EXISTS guild_metrics_daily (
        guild_id INTEGER NOT NULL,
        day INTEGER NOT NULL,
        messages_seen INTEGER NOT NULL DEFAULT 0,
        ai_reviews INTEGER NOT NULL DEFAULT 0,
        ai_compliant INTEGER NOT NULL DEFAULT 0,
        ai_gray_area INTEGER NOT NULL DEFAULT 0,
        ai_violations INTEGER NOT NULL DEFAULT 0,
        ai_errors INTEGER NOT NULL DEFAULT 0,
        ai_deleted_messages INTEGER NOT NULL DEFAULT 0,
        ai_delete_failures INTEGER NOT NULL DEFAULT 0,
        staff_review_pings INTEGER NOT NULL DEFAULT 0,
        bot_replies INTEGER NOT NULL DEFAULT 0,
        rule_source_messages INTEGER NOT NULL DEFAULT 0,
        rules_imported INTEGER NOT NULL DEFAULT 0,
        commands_used INTEGER NOT NULL DEFAULT 0,
        uploads_added INTEGER NOT NULL DEFAULT 0,
        uploads_removed INTEGER NOT NULL DEFAULT 0,
        input_tokens INTEGER NOT NULL DEFAULT 0,
        output_tokens INTEGER NOT NULL DEFAULT 0,
        total_tokens INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (guild_id, day));"
}

fn today() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .checked_div(86_400)
        .unwrap_or(0) as i64
}

fn ensure_row(conn: &Connection, guild: i64, day: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO guild_metrics_daily (guild_id, day) VALUES (?1, ?2)",
        params![guild, day],
    )?;
    Ok(())
}

pub(crate) fn increment(
    conn: &Connection,
    guild: i64,
    counter: Counter,
    amount: u64,
) -> rusqlite::Result<()> {
    if amount == 0 {
        return Ok(());
    }
    let day = today();
    ensure_row(conn, guild, day)?;
    let amount = i64::try_from(amount).unwrap_or(i64::MAX);
    conn.execute(
        &format!(
            "UPDATE guild_metrics_daily SET {column} = {column} + ?3 WHERE guild_id = ?1 AND day = ?2",
            column = counter.column()
        ),
        params![guild, day, amount],
    )?;
    Ok(())
}

pub(crate) fn add_tokens(
    conn: &Connection,
    guild: i64,
    input: u64,
    output: u64,
    total: u64,
) -> rusqlite::Result<()> {
    if input == 0 && output == 0 && total == 0 {
        return Ok(());
    }
    let day = today();
    let input = i64::try_from(input).unwrap_or(i64::MAX);
    let output = i64::try_from(output).unwrap_or(i64::MAX);
    let total =
        i64::try_from(total.max((input as u64).saturating_add(output as u64))).unwrap_or(i64::MAX);
    ensure_row(conn, guild, day)?;
    conn.execute(
        "UPDATE guild_metrics_daily SET
        input_tokens = input_tokens + ?3,
        output_tokens = output_tokens + ?4,
        total_tokens = total_tokens + ?5
        WHERE guild_id = ?1 AND day = ?2",
        params![guild, day, input, output, total],
    )?;
    Ok(())
}

fn read_totals(conn: &Connection, guild: i64, since_day: Option<i64>) -> rusqlite::Result<Totals> {
    let where_day = if since_day.is_some() {
        " AND day >= ?2"
    } else {
        ""
    };
    let sql = format!(
        "SELECT
        COALESCE(SUM(messages_seen), 0), COALESCE(SUM(ai_reviews), 0),
        COALESCE(SUM(ai_compliant), 0), COALESCE(SUM(ai_gray_area), 0),
        COALESCE(SUM(ai_violations), 0), COALESCE(SUM(ai_errors), 0),
        COALESCE(SUM(ai_deleted_messages), 0), COALESCE(SUM(ai_delete_failures), 0),
        COALESCE(SUM(staff_review_pings), 0), COALESCE(SUM(bot_replies), 0),
        COALESCE(SUM(rule_source_messages), 0), COALESCE(SUM(rules_imported), 0),
        COALESCE(SUM(commands_used), 0), COALESCE(SUM(uploads_added), 0),
        COALESCE(SUM(uploads_removed), 0),
        COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
        COALESCE(SUM(total_tokens), 0)
        FROM guild_metrics_daily WHERE guild_id = ?1{where_day}"
    );
    let mut statement = conn.prepare(&sql)?;
    let read = |row: &rusqlite::Row<'_>| -> rusqlite::Result<Totals> {
        Ok(Totals {
            messages_seen: row.get::<_, i64>(0)?.max(0) as u64,
            ai_reviews: row.get::<_, i64>(1)?.max(0) as u64,
            ai_compliant: row.get::<_, i64>(2)?.max(0) as u64,
            ai_gray_area: row.get::<_, i64>(3)?.max(0) as u64,
            ai_violations: row.get::<_, i64>(4)?.max(0) as u64,
            ai_errors: row.get::<_, i64>(5)?.max(0) as u64,
            ai_deleted_messages: row.get::<_, i64>(6)?.max(0) as u64,
            ai_delete_failures: row.get::<_, i64>(7)?.max(0) as u64,
            staff_review_pings: row.get::<_, i64>(8)?.max(0) as u64,
            bot_replies: row.get::<_, i64>(9)?.max(0) as u64,
            rule_source_messages: row.get::<_, i64>(10)?.max(0) as u64,
            rules_imported: row.get::<_, i64>(11)?.max(0) as u64,
            commands_used: row.get::<_, i64>(12)?.max(0) as u64,
            uploads_added: row.get::<_, i64>(13)?.max(0) as u64,
            uploads_removed: row.get::<_, i64>(14)?.max(0) as u64,
            input_tokens: row.get::<_, i64>(15)?.max(0) as u64,
            output_tokens: row.get::<_, i64>(16)?.max(0) as u64,
            total_tokens: row.get::<_, i64>(17)?.max(0) as u64,
        })
    };
    if let Some(day) = since_day {
        statement.query_row(params![guild, day], |row| read(row))
    } else {
        statement.query_row(params![guild], |row| read(row))
    }
}

pub(crate) fn today_totals(conn: &Connection, guild: i64) -> rusqlite::Result<Totals> {
    read_totals(conn, guild, Some(today()))
}

pub(crate) fn seven_day_totals(conn: &Connection, guild: i64) -> rusqlite::Result<Totals> {
    read_totals(conn, guild, Some(today().saturating_sub(6)))
}

pub(crate) fn all_time_totals(conn: &Connection, guild: i64) -> rusqlite::Result<Totals> {
    read_totals(conn, guild, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_and_tokens_are_aggregated() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(schema()).unwrap();
        increment(&db, 1, Counter::MessagesSeen, 2).unwrap();
        increment(&db, 1, Counter::AiViolations, 1).unwrap();
        add_tokens(&db, 1, 10, 4, 14).unwrap();
        let totals = today_totals(&db, 1).unwrap();
        assert_eq!(totals.messages_seen, 2);
        assert_eq!(totals.ai_violations, 1);
        assert_eq!(totals.total_tokens, 14);
        assert_eq!(today_totals(&db, 2).unwrap(), Totals::default());
    }
}
