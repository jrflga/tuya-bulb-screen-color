use std::io::ErrorKind::WouldBlock;
use std::{
    collections::HashMap,
    hash::Hash,
    net::IpAddr,
    str::FromStr,
    thread,
    time::{Duration, SystemTime},
};
use std::{env, path};

use clap::Parser;
use color_thief::get_palette;
use colors_transform::{Color, Hsl, Rgb};
use image::RgbaImage;
use log::{debug, error, info};
use rust_tuyapi::Payload;
use rust_tuyapi::{error::ErrorKind, PayloadStruct, TuyaDevice};
use scrap::{Capturer, Display};
use serde::Serialize;
use serde_json::json;

extern crate pretty_env_logger;

#[derive(Eq, PartialEq, Hash)]
enum DataPointsKey {
    // SwitchLed = 20,
    ColorMode = 21,
    Color = 24,
}

impl DataPointsKey {
    fn get(&self) -> String {
        match self {
            // DataPointsKey::SwitchLed => "20".to_string(),
            DataPointsKey::ColorMode => "21".to_string(),
            DataPointsKey::Color => "24".to_string(),
        }
    }
}

#[derive(clap::ValueEnum, Debug, Clone, Default, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Feature {
    #[default]
    SwitchLed,
    ColorPicker,
    WhiteMode,
    ColorMode
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[arg(long)]
    id: String,

    #[arg(long)]
    key: String,

    #[arg(long)]
    ip: String,

    #[arg(long, default_value_t = false)]
    debug: bool,

    #[arg(long)]
    mode: Feature,
}

fn main() {
    let args = Args::parse();

    if args.debug {
        env::set_var("RUST_LOG", "none,tuya_bulb_screen_color=debug");
    } else {
        env::set_var("RUST_LOG", "none,tuya_bulb_screen_color=info");
    }

    pretty_env_logger::init();

    let device = connect(args.key, args.ip);

    match args.mode {
        Feature::SwitchLed => {
            error!("Not implemented yet");
        }
        Feature::ColorPicker => {
            info!("Starting to see color on the screen...");
            color_picker(device, args.id.clone());
        }
        Feature::ColorMode => {
            info!("Changing mode to color");
            color_mode(device, args.id.clone(), "colour".to_string());
        },
        Feature::WhiteMode => {
            info!("Changing mode to white");
            color_mode(device, args.id.clone(), "white".to_string());
        }
    }
}

fn color_mode(device: Result<TuyaDevice, ErrorKind>, device_id: String, mode: String) {
    if let Ok(device) = device {
        let payload = create_color_mode_payload(device_id.clone(), mode);
        let _ = device.set(payload, 0);
    } else {
        error!("Failed to connect to the device.");
    }
}

fn color_picker(device: Result<TuyaDevice, ErrorKind>, device_id: String) {
    let mut last_color = Hsl::from(0.0, 0.0, 0.0);

    if let Ok(device) = device {
        loop {
            let dominant_color = generate_screenshot_and_get_dominant_color(false);
            let payload = create_color_picker_payload(device_id.clone(), dominant_color);
            let threshold = 10.0;

            let diff = color_diff(&last_color, &dominant_color);

            if diff <= threshold {
                info!("Color is the same, not sending payload.");
            } else {
                info!("Color is different, sending payload.");
                let _ = device.set(payload, 0);
            }

            last_color = dominant_color;

            thread::sleep(Duration::from_secs(1));
        }
    } else {
        println!("Failed to connect to the device.");
    }
}

fn connect(key: String, ip: String) -> Result<TuyaDevice, ErrorKind> {
    TuyaDevice::create("ver3.3", Some(&key), IpAddr::from_str(&ip).unwrap())
}

fn hsv2tuya(hsv: (u32, u32, u32)) -> String {
    let (h, s, v) = hsv;
    let tuya_h = format!("{:04x}", h);
    let tuya_s = format!("{:04x}", s * 10);
    let tuya_v = format!("{:04x}", v * 10);

    format!("{}{}{}", tuya_h, tuya_s, tuya_v)
}

fn generate_screenshot_and_get_dominant_color(save_image: bool) -> Hsl {
    let path = path::Path::new("./screenshots/");
    let one_second = Duration::new(1, 0);
    let one_frame = one_second / 60;
    let display = Display::all().expect("Couldn't find any display.");
    let second = display
        .into_iter()
        .next()
        .expect("Couldn't find second display.");

    let file_name = format!(
        "{}.jpeg",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );

    let mut capturer: Capturer = Capturer::new(second).expect("Failed to create capturer");
    let (w, h) = (capturer.width(), capturer.height());

    loop {
        let buffer = match capturer.frame() {
            Ok(buffer) => buffer,
            Err(error) => {
                if error.kind() == WouldBlock {
                    thread::sleep(one_frame);
                    continue;
                } else {
                    panic!("Error: {}", error);
                }
            }
        };

        let swapped_buffer = swap_color_channels(&buffer, w, h);

        debug!("Swapped color channels.");

        if save_image {
            save_screenshot(path, &file_name, &swapped_buffer, w, h);

            debug!("Saved screenshot: {}", file_name);
        } else {
            debug!("Not saving screenshot.");
        }

        let img = create_image_from_buffer(&swapped_buffer, w, h);

        debug!("Created image from buffer.");

        let dominant_color = get_dominant_color(&img);

        debug!("Dominant color: {:?}", dominant_color);

        return dominant_color.to_hsl();
    }
}

fn swap_color_channels(buffer: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut swapped_buffer = Vec::with_capacity(width * height * 4);
    for i in (0..buffer.len()).step_by(4) {
        swapped_buffer.extend_from_slice(&[buffer[i + 2], buffer[i + 1], buffer[i], buffer[i + 3]]);
    }
    swapped_buffer
}

fn save_screenshot(path: &path::Path, file_name: &str, buffer: &[u8], width: usize, height: usize) {
    image::save_buffer(
        path.join(file_name),
        buffer,
        width as u32,
        height as u32,
        image::ColorType::Rgba8,
    )
    .unwrap();
}

fn create_image_from_buffer(buffer: &[u8], width: usize, height: usize) -> RgbaImage {
    image::ImageBuffer::from_raw(width as u32, height as u32, buffer.to_vec())
        .expect("Failed to create image")
}

fn get_dominant_color(img: &RgbaImage) -> Rgb {
    let palette = get_palette(
        &img.clone().into_vec(),
        color_thief::ColorFormat::Rgba,
        10,
        2,
    )
    .unwrap();
    let dominant_color = palette.first().unwrap();

    debug!("get_dominant_color: {:?}", dominant_color);

    Rgb::from(
        dominant_color.r as f32,
        dominant_color.g as f32,
        dominant_color.b as f32,
    )
}

fn create_color_picker_payload(id: String, hsl: Hsl) -> Payload {
    let mut dps = HashMap::new();
    dps.insert(DataPointsKey::ColorMode.get(), json!("colour"));

    let lightness = if hsl.get_lightness() > 50.0 { 50 } else { 100 };

    dps.insert(
        DataPointsKey::Color.get(),
        json!(hsv2tuya((
            hsl.get_hue() as u32,
            hsl.get_saturation() as u32,
            lightness as u32
        ))),
    );
    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32;

    Payload::Struct(PayloadStruct {
        dev_id: id.to_string(),
        gw_id: Some(id.to_string()),
        uid: None,
        t: Some(current_time),
        dp_id: None,
        dps: Some(dps),
    })
}

fn create_color_mode_payload(id: String, mode: String) -> Payload {
    let mut dps = HashMap::new();
    dps.insert(DataPointsKey::ColorMode.get(), json!(mode));

    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32;

    Payload::Struct(PayloadStruct {
        dev_id: id.to_string(),
        gw_id: Some(id.to_string()),
        uid: None,
        t: Some(current_time),
        dp_id: None,
        dps: Some(dps),
    })
}

fn color_diff(color1: &Hsl, color2: &Hsl) -> f32 {
    let hue_diff = (color1.get_hue() - color2.get_hue()).abs();
    let hue_diff = if hue_diff > 180.0 {
        360.0 - hue_diff
    } else {
        hue_diff
    };

    let sat_diff = (color1.get_saturation() - color2.get_saturation()).abs();
    let lum_diff = (color1.get_lightness() - color2.get_lightness()).abs();
    hue_diff + sat_diff + lum_diff
}
