use crate::app::TimeFrame;
use chrono::{DateTime, Utc};
use yahoo_finance::{history, Bar, Profile, Quote};

#[derive(Debug, Default)]
pub struct Stock {
    pub bars: Vec<Bar>,
    pub profile: Option<Profile>,
    pub quote: Option<Quote>,
    pub symbol: String,
}

impl Stock {
    pub fn name(&self) -> Option<&str> {
        match &self.profile {
            Some(Profile::Company(company)) => Some(company.name.as_str()),
            Some(Profile::Fund(fund)) => Some(fund.name.as_str()),
            None => None,
        }
    }

    pub async fn load_historical_prices(
        &mut self,
        time_frame: TimeFrame,
        start_date: Option<DateTime<Utc>>,
        end_date: Option<DateTime<Utc>>,
    ) -> anyhow::Result<()> {
        self.bars = if let (Some(_), Some(start_date)) = (time_frame.duration(), start_date) {
            history::retrieve_range(self.symbol.as_str(), start_date, end_date).await?
        } else {
            history::retrieve_interval(self.symbol.as_str(), time_frame.interval()).await?
        };

        Ok(())
    }

    pub async fn load_profile(&mut self) -> anyhow::Result<()> {
        self.profile = Some(Profile::load(self.symbol.as_str()).await?);

        Ok(())
    }
}
