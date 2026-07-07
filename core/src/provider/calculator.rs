use std::future::Future;
use std::pin::Pin;

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;
use anyhow::Result;

pub struct Calculator;

impl Plugin for Calculator {
    fn meta(&self) -> &Meta {
        &Meta {
            id: "calc",
            name: "Calculator",
            icon: "calc",
            keyword: "",
        }
    }

    fn search(
        &self,
        _query: &str,
        full: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
        let expr = full.to_string();
        Box::pin(async move { do_search(&expr) })
    }
}

fn do_search(expr: &str) -> Result<Vec<ResultItem>> {
    if expr.is_empty() {
        return Ok(vec![]);
    }

    match meval::eval_str(expr) {
        Ok(value) => {
            if value.is_infinite() || value.is_nan() {
                return Ok(vec![]);
            }
            let formatted = if value.fract() == 0.0 {
                format!("{}", value as i64)
            } else {
                format!("{:.10}", value)
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            };

            Ok(vec![ResultItem {
                title: formatted,
                summary: Some(expr.to_string()),
                on_click: None,
                icon: find_icon_path("calc"),
            }])
        }
        Err(_) => Ok(vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(expr: &str) -> String {
        do_search(expr)
            .unwrap()
            .first()
            .map(|r| r.title.clone())
            .unwrap_or_default()
    }

    #[test]
    fn basic_arithmetic() {
        assert_eq!(first("2 + 3"), "5");
        assert_eq!(first("10 - 7"), "3");
        assert_eq!(first("6 * 4"), "24");
        assert_eq!(first("15 / 3"), "5");
    }

    #[test]
    fn empty_input() {
        assert!(do_search("").unwrap().is_empty());
    }

    #[test]
    fn division_by_zero() {
        assert!(do_search("1/0").unwrap().is_empty());
    }

    #[test]
    fn non_math_input() {
        assert!(do_search("firefox").unwrap().is_empty());
        assert!(do_search("hello world").unwrap().is_empty());
    }

    #[test]
    fn decimals() {
        let result = first("3.14 * 2");
        assert!(result.starts_with("6.28"));
    }

    #[test]
    fn negative() {
        assert_eq!(first("-5 + 8"), "3");
        assert_eq!(first("0 - 10"), "-10");
    }

    #[test]
    fn power() {
        assert_eq!(first("2 ^ 10"), "1024");
    }
}
