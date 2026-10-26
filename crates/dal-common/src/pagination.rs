use serde::{Deserialize, Deserializer, Serialize};

/// Query parameters for paginated list endpoints.
#[derive(Debug, Clone, Deserialize)]
pub struct PageParams {
    #[serde(
        default = "PageParams::default_page",
        deserialize_with = "deserialize_query_u32"
    )]
    pub page: u32,
    #[serde(
        default = "PageParams::default_per_page",
        deserialize_with = "deserialize_query_u32"
    )]
    pub per_page: u32,
}

impl PageParams {
    fn default_page() -> u32 {
        1
    }
    fn default_per_page() -> u32 {
        25
    }

    /// Returns a validated (page, per_page) tuple.
    /// - page   is clamped to ≥ 1
    /// - per_page is clamped to 1..=100
    pub fn validated(&self) -> (u32, u32) {
        let page = self.page.max(1);
        let per_page = self.per_page.clamp(1, 100);
        (page, per_page)
    }

    /// LIMIT for SQL query.
    pub fn limit(&self) -> i64 {
        let (_, per_page) = self.validated();
        per_page as i64
    }

    /// OFFSET for SQL query.
    pub fn offset(&self) -> i64 {
        let (page, per_page) = self.validated();
        i64::from(page - 1) * i64::from(per_page)
    }
}

/// A paginated page of results.
#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub per_page: u32,
    pub total: i64,
    pub pages: u32,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>, params: &PageParams, total: i64) -> Self {
        let (page, per_page) = params.validated();
        let pages = ((total as f64) / (per_page as f64)).ceil() as u32;
        Self {
            items,
            page,
            per_page,
            total,
            pages,
        }
    }
}

fn deserialize_query_u32<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum QueryNumber {
        Number(u32),
        Text(String),
    }

    match QueryNumber::deserialize(deserializer)? {
        QueryNumber::Number(value) => Ok(value),
        QueryNumber::Text(value) => value.parse::<u32>().map_err(serde::de::Error::custom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_pagination_accepts_the_full_query_parameter_domain() {
        let params = PageParams {
            page: u32::MAX,
            per_page: 100,
        };
        assert_eq!(params.offset(), 429_496_729_400);
        assert_eq!(params.limit(), 100);
        assert_eq!(
            PageParams {
                page: 0,
                per_page: 0
            }
            .offset(),
            0
        );
    }
}
