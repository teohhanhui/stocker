use crate::app::TimeFrame;
use chrono::{DateTime, Utc};
use std::error::Error;
use yahoo_finance::{history, Bar, Profile, Quote};

#[derive(Debug)]
pub struct Stock {
    pub bars: Vec<Bar>,
    pub profile: Option<Profile>,
    pub quote: Option<Quote>,
    pub symbol: String,
}

impl Stock {
    pub async fn load_historical_prices(
        &mut self,
        time_frame: TimeFrame,
        end_date: DateTime<Utc>,
    ) -> Result<(), Box<dyn Error>> {
        self.bars = if let Some(duration) = time_frame.duration() {
            history::retrieve_range(self.symbol.as_str(), end_date - duration, Some(end_date))
                .await?
        } else {
            history::retrieve_interval(self.symbol.as_str(), time_frame.interval()).await?
        };

        Ok(())
    }

    pub async fn load_profile(&mut self) -> Result<(), Box<dyn Error>> {
        self.profile = Some(Profile::load(self.symbol.as_str()).await?);

        Ok(())
    }
}
