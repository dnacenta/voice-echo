use chrono::{Local, Timelike};
use rand::seq::SliceRandom;

const ANYTIME: &[&str] = &[
    "Hey, it's {name}",
    "Hi there, {name} here",
    "Hello, this is {name}",
    "{name} here, what's up?",
];

const MORNING: &[&str] = &["Good morning, {name} here", "Morning! It's {name}"];

const AFTERNOON: &[&str] = &[
    "Good afternoon, it's {name}",
    "Hey, good afternoon, {name} here",
];

const EVENING: &[&str] = &["Good evening, this is {name}", "Evening! {name} here"];

const NIGHT: &[&str] = &[
    "Hey, it's late, but {name}'s here",
    "{name} here, burning the midnight oil?",
];

fn time_pool(hour: u32) -> &'static [&'static str] {
    match hour {
        5..=11 => MORNING,
        12..=16 => AFTERNOON,
        17..=20 => EVENING,
        _ => NIGHT,
    }
}

/// Select a greeting based on the current time of day.
///
/// Combines anytime greetings with time-specific ones and picks randomly.
/// The `{name}` placeholder is replaced with the provided name.
pub fn select_greeting(name: &str) -> String {
    let hour = Local::now().hour();
    select_greeting_for_hour(name, hour)
}

fn select_greeting_for_hour(name: &str, hour: u32) -> String {
    let time_specific = time_pool(hour);
    let mut pool: Vec<&str> = Vec::with_capacity(ANYTIME.len() + time_specific.len());
    pool.extend_from_slice(ANYTIME);
    pool.extend_from_slice(time_specific);

    let mut rng = rand::thread_rng();
    let template = pool.choose(&mut rng).unwrap_or(&ANYTIME[0]);
    template.replace("{name}", name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting_contains_name() {
        let greeting = select_greeting_for_hour("TestBot", 10);
        assert!(
            greeting.contains("TestBot"),
            "greeting should contain entity name: {greeting}"
        );
    }

    #[test]
    fn greeting_no_placeholder_leftover() {
        for hour in 0..24 {
            let greeting = select_greeting_for_hour("Echo", hour);
            assert!(
                !greeting.contains("{name}"),
                "placeholder not replaced at hour {hour}: {greeting}"
            );
        }
    }

    #[test]
    fn greeting_never_empty() {
        for hour in 0..24 {
            let greeting = select_greeting_for_hour("X", hour);
            assert!(!greeting.is_empty(), "empty greeting at hour {hour}");
        }
    }

    #[test]
    fn time_pool_morning() {
        let pool = time_pool(8);
        assert!(pool
            .iter()
            .any(|g| g.contains("morning") || g.contains("Morning")));
    }

    #[test]
    fn time_pool_afternoon() {
        let pool = time_pool(14);
        assert!(pool.iter().any(|g| g.contains("afternoon")));
    }

    #[test]
    fn time_pool_evening() {
        let pool = time_pool(19);
        assert!(pool
            .iter()
            .any(|g| g.contains("evening") || g.contains("Evening")));
    }

    #[test]
    fn time_pool_night() {
        let pool = time_pool(23);
        assert!(pool
            .iter()
            .any(|g| g.contains("late") || g.contains("midnight")));
    }

    #[test]
    fn time_pool_boundaries() {
        // 4 AM = night, 5 AM = morning, 11 AM = morning, 12 PM = afternoon
        assert_eq!(time_pool(4), NIGHT);
        assert_eq!(time_pool(5), MORNING);
        assert_eq!(time_pool(11), MORNING);
        assert_eq!(time_pool(12), AFTERNOON);
        assert_eq!(time_pool(16), AFTERNOON);
        assert_eq!(time_pool(17), EVENING);
        assert_eq!(time_pool(20), EVENING);
        assert_eq!(time_pool(21), NIGHT);
    }
}
