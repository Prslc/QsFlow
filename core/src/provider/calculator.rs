use anyhow::Result;

use crate::models::ResultItem;
use crate::system::icon::find_icon_path;

pub fn calculate(expr: &str) -> Result<Vec<ResultItem>> {
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
                icon: find_icon_path("calculator"),
            }])
        }
        Err(_) => Ok(vec![]),
    }
}
