use rml_rtmp::sessions::ClientSessionConfig;

#[derive(Clone)]
pub struct MyClientSessionConfig {
    pub config: ClientSessionConfig,
    stream_key: Option<String>,
}

static YOUTUBE_CHUNK_SIZE: u32 = 128;

//static YOUTUBE_DEFAULT_SERVER: &str = "rtmp://a.rtmp.youtube.com:1935";
static YOUTUBE_DEFAULT_SERVER: &str = "a.rtmp.youtube.com:1935";
static YOUTUBE_APP: &str = "live2/x";
static YOUTUBE_KEY: &str = "0kjx-g7uh-82dh-vbqc-ct1p";
//static YOUTUBE_APP_OPTION: &str = " app=live2";
static YOUTUBE_APP_OPTION: &str = "live2";

/*
        defaultProtocol = Protocol::RTMP;
        defaultServer = "a.rtmp.youtube.com";
        mApp = "live2/x";
        if (APP_OPTIONS_IN_URL)
        {
            mOptions = " app=live2";
        }
        else
        {
            mAppOption = "live2";
        }

void StreamingConfig::composeURL()
{
    mURL = std::string(protocolString()).append(mServer)
                        .append("/").append(mApp)
                        .append("/").append(mKey)
                        .append(mOptions);
}
*/

impl MyClientSessionConfig {
    fn new() -> Self {
        Self {
            config: ClientSessionConfig::new(),
            stream_key: None,
        }
    }

    pub fn default() -> Self {
        let mut var = Self::new();
        var.set_url(YOUTUBE_DEFAULT_SERVER);
        var.set_stream_key(Some(YOUTUBE_KEY));
        return var;
    }

    pub fn set_url(&mut self, tc_url: &str) {
        self.config.tc_url = Some(tc_url.to_string());
    }

    pub fn get_app(&self) -> String {
        YOUTUBE_APP.to_string()
    }

    pub fn get_url(&self) -> Option<String> {
        self.config.tc_url.clone()
    }

    pub fn set_chunk_size(&mut self, chunk_size: u32) {
        self.config.chunk_size = chunk_size;
    }

    pub fn set_stream_key(&mut self, stream_key: Option<&str>) {
        if stream_key.is_some() {
            self.stream_key = Some(stream_key.unwrap().to_string());
        } else {
            self.stream_key = None;
        }
    }

    pub fn get_stream_key(&self) -> Option<String> {
        return self.stream_key.clone();
    }

    pub fn set_playback_buffer_len(&mut self, buffer_len: u32) {
        self.config.playback_buffer_length_ms = buffer_len;
    }

    pub fn set_window_ack_size(&mut self, window_ack_size: u32) {
        self.config.window_ack_size = window_ack_size;
    }

    pub fn set_flash_version(&mut self, flash_version: &str) {
        self.config.flash_version = flash_version.to_string();
    }
}

