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
    let biz_time_start = 900;
    let biz_time_end = 1700;
    let highlight_biz_time = false;

    html! {
    <div class="container">
        <div class="row">
            <center><h1>{ "Find the best time to meet across timezones"}</h1>
            <p>{"Built with 💪 of Rust and 💗 of "} <a href="https://twitter.com/shantanugoel">{"shantanugoel"}</a>
             {" ("} <a href="https://github.com/shantanugoel/timetomeet-rs">{"Source Code"}</a> {")"}</p>
            </center>
        </div>
        <div class="row">
            <div class="one-third column">
                <h4>{"How to use this:"}</h4>
                <ul>
                    <li> {"Pick the meeting date. Table will auto update whenever a new date is picked."} </li>
                    <li> {"Search for a city. Results appear as buttons on right as you search. This can be repeated as many times as needed."} </li>
                    <li> {"Click on a city name to add it to the table. This can be repeated as many times as needed."} </li>
                    <li> {"The table shows the time overlaps across the cities added."} </li>
                    <li> {"Click on the `x` near a city name in the table header to remove it from the table."} </li>
                </ul>
            </div>
            <div class="one-third column">
                <h4> {"1. Pick the meeting date"} </h4>
                <input class="u-full-width" type="date" onchange=model.link.callback(Msg::DatePick)/>
                <h4> {"2. Search for cities"} </h4>
                <input class="u-full-width" placeholder="Enter city name to search/add" type="search" oninput=model.link.callback(Msg::CityInput)/>
            </div>
            <div class="one-third column">
                <h4> {"3. Click to add"} </h4>
                {
                    for model.props.search_results.clone().into_iter().map(|s| {let y = s.clone(); html!{
                        <input type="button" class="button-primary" value={s} onclick=model.link.callback(move |_| {Msg::CityAdd(y.clone())}) />
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
            for cities.iter().enumerate().map(|(idx, city)| {html! {<th>
                {city}
                <sup><a href="#" onclick=model.link.callback(move |_| Msg::CityRemove(idx))>{" X "}</a></sup>
                </th>} })
            }
            </tr>
            </thead>
            {
                for hours_range.map(|x| {html! {<tr>
                    {
                        for tzs.clone().into_iter().map(|t| {
                            let time = t + Duration::hours(x);
                            let mut time_class = "off";
                            if highlight_biz_time {
                                let time_int: u32 = time.format("%H%M").to_string().parse().unwrap();
                                if time_int >= biz_time_start && time_int <= (biz_time_end - 100) {
                                    time_class = "biz";
                            }
                            }
                            html!{
                            <td class={ time_class }>{ time.format("%l:%M %p %e %b") }</td>
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
