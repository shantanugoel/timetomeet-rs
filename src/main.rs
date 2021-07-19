mod model;
mod utils;
mod views;

use model::Model;

fn main() {
    yew::start_app::<Model>();
}
