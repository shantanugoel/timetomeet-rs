use std::str::FromStr;

use chrono::NaiveDate;
use simsearch::{SearchOptions, SimSearch};
use yew::{prelude::*, services::ConsoleService};

use crate::{utils::today, views::main_view};

pub enum Msg {
    DatePick(ChangeData),
    CityInput(InputData),
    CityAdd(String),
    CityRemove(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelProps {
    pub ref_date: NaiveDate,
    pub search_results: Vec<String>,
    pub selected_cities: Vec<String>,
}

impl Default for ModelProps {
    fn default() -> Self {
        ModelProps {
            ref_date: NaiveDate::from_str(today().as_str()).unwrap(),
            search_results: vec![
                String::from_str("New Delhi,IN,Asia/Kolkata").unwrap(),
                String::from_str("Seattle,US,America/Los_Angeles").unwrap(),
                String::from_str("New York City,US,America/New_York").unwrap(),
                String::from_str("London,GB,Europe/London").unwrap(),
                String::from_str("Sydney,AU,Australia/Sydney").unwrap(),
            ],
            selected_cities: vec![
                String::from_str("Bengaluru,IN,Asia/Kolkata").unwrap(),
                String::from_str("Mountain View,US,America/Los_Angeles").unwrap(),
                String::from_str("Shanghai,CN,Asia/Shanghai").unwrap(),
                String::from_str("Singapore,SG,Asia/Singapore").unwrap(),
            ],
        }
    }
}
pub struct Model {
    pub link: ComponentLink<Self>,
    tz_db: SimSearch<String>,
    pub props: ModelProps,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_props: Self::Properties, link: ComponentLink<Self>) -> Self {
        let cities_data = include_str!("./data/cities.csv");
        let mut tz_db: SimSearch<String> =
            SimSearch::new_with(SearchOptions::new().stop_whitespace(false));
        for line in cities_data.lines() {
            let data: Vec<&str> = line.splitn(3, ',').collect();
            tz_db.insert(line.to_string(), data[0]);
        }

        let props = ModelProps::default();

        Self { link, tz_db, props }
    }

    fn update(&mut self, msg: Self::Message) -> yew::ShouldRender {
        let mut result = false;
        match msg {
            Msg::DatePick(d) => match d {
                ChangeData::Value(date_string) => {
                    self.props.ref_date = NaiveDate::from_str(date_string.as_str()).unwrap();
                    result = true;
                }
                _ => ConsoleService::log("Incorrect message type received while picking date"),
            },

            Msg::CityInput(city_partial) => {
                self.props.search_results.clear();
                let results: Vec<String> = self
                    .tz_db
                    .search(city_partial.value.as_ref())
                    .iter()
                    .take(6)
                    .cloned()
                    .collect();
                self.props.search_results.extend(results);
                result = true;
            }

            Msg::CityAdd(data) => {
                let existing_index = self.props.selected_cities.iter().position(|x| x.eq(&data));
                if existing_index.is_none() {
                    self.props.selected_cities.push(data);
                }
                result = true
            }

            Msg::CityRemove(idx) => {
                self.props.selected_cities.remove(idx);
                result = true
            }
        }

        result
    }

    fn change(&mut self, _props: Self::Properties) -> yew::ShouldRender {
        false
    }

    fn view(&self) -> yew::Html {
        main_view(self)
    }
}
