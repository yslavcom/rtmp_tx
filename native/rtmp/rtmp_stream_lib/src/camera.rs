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