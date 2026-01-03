use ffmpeg_next::{self as ffmpeg, format, media::Type, Error};

enum Direction {
    Forward,
    Backward,
}

pub struct Saw {
    ictx: format::context::Input,
    octx: format::context::Output,
    stream_map: Vec<usize>,
    pub start: f64,
    pub first_kf: Option<f64>,
    pub last_kf: Option<f64>,
    end: f64,
}

impl std::fmt::Debug for Saw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Streams: \n")?;
        for stream in self.ictx.streams() {
            let info = format!("{:?}", stream.parameters().medium());
            let id = format!("{:?}", stream.parameters().id());
            write!(f, "\nType: {{")?;
            write!(f, " {}", info)?;
            write!(f, "  {} ", id)?;
            write!(f, "}}\n")?;
        }
        // Write raw multiline string
        writeln!(f, "  start: {:?},", &self.start)?;
        writeln!(f, "  first_kf: {:?},", &self.first_kf)?;
        writeln!(f, "  last_kf: {:?},", &self.last_kf)?;
        writeln!(f, "  end: {:?},", &self.end)
    }
}

impl Saw {
    pub fn new(input: &str, output: &str, start: f64, end: f64) -> Result<Self, Error> {
        ffmpeg::init()?;
        let ictx = format::input(&input)?;
        let mut octx = format::output(&output)?;
        let mut stream_map = Vec::new();
        for istream in ictx.streams() {
            let codec_id = istream.parameters().id();
            let codec = ffmpeg::codec::decoder::find(codec_id).take();
            let mut ostream = octx.add_stream(codec)?;
            ostream.set_parameters(istream.parameters());
            stream_map.push(istream.index());
        }
        octx.write_header()?;
        Ok(Saw {
            ictx,
            octx,
            stream_map,
            start,
            first_kf: None,
            last_kf: None,
            end,
        })
    }

    pub fn seek(&mut self) -> Result<(), Error> {
        self.first_kf =
            self.find_closest_keyframe_inside_boundaries(self.start, Direction::Forward)?;
        self.last_kf =
            self.find_closest_keyframe_inside_boundaries(self.end, Direction::Backward)?;
        Ok(())
    }

    fn find_closest_keyframe_inside_boundaries(
        &mut self,
        target_time_seconds: f64,
        direction: Direction,
    ) -> Result<Option<f64>, Error> {
        let stream = self
            .ictx
            .streams()
            .best(Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;

        let time_base = stream.time_base();
        let stream_index = stream.index();

        // Convert target time to stream time base units
        let target_ts = (target_time_seconds / f64::from(time_base)) as i64;

        let direction = match direction {
            Direction::Forward => ffmpeg::ffi::AVSEEK_FLAG_FRAME,
            Direction::Backward => ffmpeg::ffi::AVSEEK_FLAG_BACKWARD,
        };
        // Seek to nearest keyframe BEFORE or AT target_ts
        unsafe {
            ffmpeg::ffi::av_seek_frame(
                self.ictx.as_mut_ptr(),
                stream_index as i32,
                target_ts,
                direction,
            );
        }

        // Read packets forward until we find the first keyframe
        for (stream, packet) in self.ictx.packets() {
            if stream.index() != stream_index {
                continue;
            }

            if packet.is_key() {
                let ts = packet.pts().or_else(|| packet.dts()).unwrap();
                let keyframe_time = (ts as f64) * f64::from(time_base);
                return Ok(Some(keyframe_time));
            }
        }

        unsafe {
            ffmpeg::ffi::avformat_flush(self.ictx.as_mut_ptr());
        }
        Ok(None)
    }
}
