use chrono::{DateTime, Duration, TimeZone};
use chrono_tz::Tz;
use yew::{prelude::*, Html};

use crate::model::{Model, Msg};

pub fn main_view(model: &Model) -> Html {
    let mut cities: Vec<String> = Vec::new();
    let mut tzs: Vec<DateTime<Tz>> = Vec::new();
    let mut parsing_first_city = true;
    for record in model.props.selected_cities.iter() {
        let data: Vec<&str> = record.splitn(3, ',').collect();
        cities.push(data[0].to_string());
        let tz: Tz = data[2].parse().unwrap();
        if parsing_first_city {
            let time = tz
                .from_local_date(&model.props.ref_date)
                .unwrap()
                .and_hms(9, 0, 0);
            tzs.push(time);
            parsing_first_city = false;
        } else {
            let time = tzs[0].with_timezone(&tz);
            tzs.push(time);
        }
    }
    let hours_range = 0..24;

    html! {
    <div class="container">
        <div class="row">
            <div class="four columns">
                <input class="u-full-width" type="date" onchange=model.link.callback(Msg::DatePick)/>
            </div>
            <div class="four columns">
                <input class="u-full-width" oninput=model.link.callback(Msg::CityInput)/>
            </div>
            <div class="four columns">
                {
                    for model.props.search_results.clone().into_iter().map(|s| {html!{
                        <p> {s } </p>
                    }})
                }
            </div>
        </div>
        <div class="row">
        <div class="tweleve columns">
            <table class="u-full-width">
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
        </div>
        </div>
    </div>
    }
}
