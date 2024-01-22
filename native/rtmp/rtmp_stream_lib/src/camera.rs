use rscam::{Camera, Config, Frame};

use crossbeam_channel::unbounded;
use crossbeam_channel::Receiver;
use std::thread;

static X_RES: u32 = 1280;
static Y_RES: u32 = 720;
static FRAME_RATE: (u32, u32) = (1, 30); // 30 fps.
static DEFAULT_BITRATE: u32 = 2_000_000;



fn create_camera() -> Option<Camera> {
    let mut camera = Camera::new("/dev/video0").unwrap();

    let res = camera.start(&Config {
        interval: FRAME_RATE,
        resolution: (X_RES, Y_RES),
        format: b"RGB3", // b"YU12",  // b"MJPG",
        ..Default::default()
    });
    if let Err(_) = res {
        return None;
    }

    let formats = camera.formats();
    for format in formats {
        if let Ok(format) = format {
            println!("description: {}", format.description);
            let fourcc = std::str::from_utf8(&format.format).unwrap();
            println!("fourcc:{}", fourcc);
        }
    }

    Some(camera)
}

pub fn start_canera() {
    let (tx_cam, rx_enc) = unbounded();
    thread::spawn(move || {
        encode(rx_enc);
    });

    let camera = create_camera().unwrap();

    loop {
        let frame = camera.capture().unwrap();
        let yuv_source = MyFrame::build(&frame, frame.get_timestamp());

        tx_cam.send(yuv_source);
    }
}