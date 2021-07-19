use std::str::FromStr;

use chrono::NaiveDate;
use simsearch::SimSearch;
use yew::{prelude::*, services::ConsoleService};

use crate::{utils::today, views::main_view};

pub enum Msg {
    DatePick(ChangeData),
    CityInput(InputData),
}

#[derive(Default, Properties, Clone, PartialEq)]
pub struct ModelProps {
    #[prop_or_default]
    pub ref_date: Date,
    #[prop_or(vec![String::from_str("Bengaluru,IN,Asia/Kolkata").unwrap(),
     String::from_str("Mountain View,US,America/Los_Angeles").unwrap()])]
    pub current_results: Vec<String>,
}

#[derive(Clone, PartialEq)]
pub struct Date(pub NaiveDate);

impl Default for Date {
    fn default() -> Self {
        ConsoleService::log(&today());
        let date = NaiveDate::from_str(today().as_str()).unwrap();
        Date { 0: date }
    }
}
pub struct Model {
    pub link: ComponentLink<Self>,
    tz_db: SimSearch<String>,
    pub props: ModelProps,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ModelProps;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        let cities_data = include_str!("./data/cities.csv");
        let mut tz_db: SimSearch<String> = SimSearch::new();
        for line in cities_data.lines() {
            tz_db.insert(line.to_string(), line);
        }

        let model = Self { link, tz_db, props };

        model
    }

    fn update(&mut self, msg: Self::Message) -> yew::ShouldRender {
        match msg {
            Msg::DatePick(d) => match d {
                ChangeData::Value(date_string) => {
                    self.props.ref_date = Date(NaiveDate::from_str(date_string.as_str()).unwrap());
                    return true;
                }
                _ => ConsoleService::log("Incorrect message type received while picking date"),
            },

            Msg::CityInput(city_partial) => {
                self.props.current_results.clear();
                let results: Vec<String> = self
                    .tz_db
                    .search(city_partial.value.as_ref())
                    .iter()
                    .take(5)
                    .map(|s| s.clone())
                    .collect();
                self.props.current_results.extend(results);
                return true;
            }
        }
        false
    }

    fn change(&mut self, _props: Self::Properties) -> yew::ShouldRender {
        false
    }

    fn view(&self) -> yew::Html {
        main_view(self)
    }
}
