use crate::app::TimeFrame;
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
    ) -> Result<(), Box<dyn Error>> {
        self.bars = history::retrieve_interval(self.symbol.as_str(), time_frame.interval()).await?;

        Ok(())
    }

    pub async fn load_profile(&mut self) -> Result<(), Box<dyn Error>> {
        self.profile = Some(Profile::load(self.symbol.as_str()).await?);

        Ok(())
    }
}
