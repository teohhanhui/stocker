use im::OrdSet;
use yahoo_finance::{Bar, Profile, Quote};

#[derive(Clone, Debug, Default)]
pub struct Stock {
    pub bars: OrdSet<Bar>,
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
}
