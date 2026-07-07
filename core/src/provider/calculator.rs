use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;

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
