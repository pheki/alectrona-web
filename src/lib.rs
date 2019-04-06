use std::error::Error;

extern crate toml;

extern crate wasm_bindgen;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[macro_use]
extern crate lazy_static;

extern crate serde;
use serde::Deserialize;

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use std::sync::Mutex;

use alectrona::data::LogoBin;
use alectrona::image;
use alectrona::DeviceFamily;

use image::GenericImageView;

use web_sys::{Element, Node};

const EXTENSION: &str = "png";

lazy_static! {
    static ref POSSIBLE_DEVICES: HashMap<String, Device> = toml::from_str(alectrona::DEVICES_TOML).unwrap();
    // TODO use Arc<Mutex> and remove some clone()s
    static ref SELECTED_DEVICE: Mutex<Option<Device>> = Mutex::new(None);
    static ref SELECTED_LOGO_BIN: Mutex<Option<LogoBin>> = Mutex::new(None);
    static ref SELECTED_LOGO_ID: Mutex<Option<String>> = Mutex::new(None);
}

#[wasm_bindgen(module = "/static/dom_manipulator.js")]
extern "C" {
    fn show_image(image: &[u8], extension: &str, logo_id: Option<&str>);
    fn set_logo_list(identifier_array: Vec<JsValue>);
    fn reset_logo_list();
    fn enable_bin_input();
    fn enable_replace();
    fn enable_download();
}

#[derive(Clone, Deserialize)]
struct Device {
    name: String,
    family: DeviceFamily,
    #[allow(dead_code)]
    width: u32,
    #[allow(dead_code)]
    height: u32,
}

/// Initializes devices using POSSIBLE_DEVICES, creating an <option> element on <select id="select-device"> for each device.
#[wasm_bindgen]
pub fn init_devices() {
    console_error_panic_hook::set_once();
    let document = web_sys::window().unwrap().document().unwrap();
    let select_element: Node = document.get_element_by_id("select-device").unwrap().into();

    let mut dummy_option: Option<Element> = None;
    let child_nodes = select_element.child_nodes();
    for i in 0..child_nodes.length() {
        if let Some(node) = child_nodes.get(i) {
            if let Ok(element) = node.dyn_into::<Element>() {
                dummy_option = Some(element);
            }
        } else {
            break;
        }
    }
    let dummy_option = dummy_option.unwrap();

    let mut possible_devices: Vec<(&String, &Device)> = POSSIBLE_DEVICES.iter().collect();
    possible_devices.sort_unstable_by_key(|(_codename, device)| &device.name);

    for (codename, device) in possible_devices {
        let new_option: Element = dummy_option
            .clone_node_with_deep(true)
            .unwrap()
            .dyn_into()
            .unwrap();
        new_option.remove_attribute("disabled").unwrap();
        new_option.remove_attribute("selected").unwrap();
        new_option.set_attribute("value", codename).unwrap();
        new_option.set_text_content(Some(&device.name));
        select_element.append_child(&new_option).unwrap();
    }
}

/// Saves selected device and enables the binary input field.
#[wasm_bindgen]
pub fn handle_device(codename: &str) {
    // TODO handle codename not found
    *SELECTED_DEVICE.lock().unwrap() = Some(POSSIBLE_DEVICES[codename].clone());
    enable_bin_input();
}

/// Parses the logo.bin file, saving it on SELECTED_LOGO_BIN and creating the list of logo_ids to select.
#[wasm_bindgen]
pub fn handle_file(buffer: &[u8]) -> Result<String, JsValue> {
    let mut incursor = Cursor::new(buffer);
    let family = SELECTED_DEVICE.lock().unwrap().clone().unwrap().family;
    let logo_bin = LogoBin::from_file(&mut incursor, family.clone())
        .map_err::<JsValue, _>(|err| err.description().into())?;
    let text = format!("{}", logo_bin);
    reset_logo_list();
    let logo_identifier_vec: Vec<JsValue> = logo_bin
        .logos()
        .iter()
        .map(|logo| JsValue::from_str(logo.identifier()))
        .collect();
    set_logo_list(logo_identifier_vec);
    *SELECTED_LOGO_BIN.lock().unwrap() = Some(logo_bin);
    Ok(text)
}

/// Extracts the image with the id selected and shows it on the <img> element.
#[wasm_bindgen]
pub fn handle_logo_id(logo_id: String) {
    let logo_bin_guard = SELECTED_LOGO_BIN.lock().unwrap();
    // Unwraps logo_bin reference inside MutexGuard
    let logo_bin = if let Some(ref logo) = *logo_bin_guard {
        logo
    } else {
        panic!()
    };
    let image_vec = extract_logo(logo_bin, &logo_id);
    show_image(&image_vec, EXTENSION, Some(&logo_id));
    enable_replace();
    *SELECTED_LOGO_ID.lock().unwrap() = Some(logo_id);
}

/// Replaces image with the selected logo_id in the logo.bin file.
#[wasm_bindgen]
pub fn handle_image(buffer: &[u8], filename: String) -> Result<(), JsValue> {
    let extension = Path::new(&filename)
        .extension()
        .ok_or("No extension found in file.")?;
    let extension = extension.to_str().ok_or("Extension invalid")?;
    let format = match &extension[..] {
        "ico" => image::ICO,
        "jpg" | "jpeg" => image::JPEG,
        "png" => image::PNG,
        "bmp" => image::BMP,
        format => return Err(format!("Unsupported image format image/{:?}", format).into()),
    };

    let input_cursor = Cursor::new(buffer);
    let input_image = image::load(input_cursor, format)
        .map_err(|err| format!("Failed to load image: {}", err.description()))?;

    // Checks image dimensions
    let device = SELECTED_DEVICE.lock().unwrap().clone().unwrap();
    if input_image.width() != device.width || input_image.height() != device.height {
        web_sys::window().unwrap().alert_with_message(&format!("Warning: Image have wrong dimensions {}x{}, expected {}x{}. Be careful when flashing the generated image to your device.", input_image.width(), input_image.height(), device.width, device.height)).unwrap();
    }

    let mut logo_bin_guard = SELECTED_LOGO_BIN.lock().unwrap();
    // Unwraps logo_bin reference inside MutexGuard
    let logo_bin = if let Some(ref mut logo) = *logo_bin_guard {
        logo
    } else {
        panic!()
    };
    let logo_id_guard = SELECTED_LOGO_ID.lock().unwrap();
    let logo_id = if let Some(ref logo_id) = *logo_id_guard {
        logo_id
    } else {
        panic!()
    };

    // Actually replaces image in LogoBin
    logo_bin
        .replace_logo_with_id(input_image, logo_id)
        .map_err::<JsValue, _>(|err| err.description().into())?;

    let image_vec = extract_logo(logo_bin, logo_id);
    show_image(&image_vec, EXTENSION, Some(logo_id));

    Ok(())
}

/// Exports the new logo.bin file for download.
#[wasm_bindgen]
pub fn export_logo_bin() -> Result<Vec<u8>, JsValue> {
    let mut logo_bin_guard = SELECTED_LOGO_BIN.lock().unwrap();
    let logo_bin = if let Some(ref mut logo) = *logo_bin_guard {
        logo
    } else {
        panic!()
    };
    let mut cursor = Cursor::new(Vec::new());
    logo_bin
        .write_to_file(&mut cursor)
        .map_err(|_err| "Error writing to file")?;
    Ok(cursor.into_inner())
}

fn extract_logo(logo_bin: &LogoBin, logo_id: &str) -> Vec<u8> {
    let mut out_cursor = Cursor::new(Vec::new());
    logo_bin
        .extract_logo_with_id_to_file(logo_id, &mut out_cursor, EXTENSION)
        .unwrap();
    out_cursor.into_inner()
}
