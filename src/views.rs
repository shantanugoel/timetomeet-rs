use chrono::{DateTime, Duration, TimeZone};
use chrono_tz::Tz;
use yew::{prelude::*, Html};

use crate::model::{Model, Msg};

pub fn main_view(model: &Model) -> Html {
    let mut cities: Vec<String> = Vec::new();
    let mut tzs: Vec<DateTime<Tz>> = Vec::new();
    for record in model.props.selected_cities.iter() {
        let data: Vec<&str> = record.splitn(3, ',').collect();
        cities.push(data[0].to_string());
        let tz: Tz = data[2].parse().unwrap();
        let time = tz
            .from_local_date(&model.props.ref_date.0)
            .unwrap()
            .and_hms(9, 0, 0);
        tzs.push(time);
    }
    let hours_range = 0..24;

    html! {
    <div>
        <input type="date" onchange=model.link.callback(Msg::DatePick)/>
        <table>
        <thead>
        <tr>
        {
        for cities.iter().map(|city| {html! {<th> {city} </th>} })
        }
        </tr>
        </thead>
        {
            for hours_range.map(|x| {html! {<tr>
                {
                    for tzs.clone().into_iter().map(|t| { html!{
                        <td>{ (t + Duration::hours(x)).format("%l:%M %p %e %b") }</td>
                    }})
                }
                </tr>}})
        }
        </table>
        <input oninput=model.link.callback(Msg::CityInput)/>
        {
            for model.props.search_results.clone().into_iter().map(|s| {html!{
                <p> {s } </p>
            }})
        }
    </div>
    }
}
