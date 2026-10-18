use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn today() -> String {
    let date: String = js_sys::Date::new_0().to_iso_string().into();
    date[..10].into()
}
