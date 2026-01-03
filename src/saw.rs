use failure::Error;
use ffmpeg::{codec, format, media, media::Type};
use ffmpeg_next as ffmpeg;

enum Direction {
    Forward,
    Backward,
}

struct Boundaries {
    start: f64,
    end: f64,
}

pub struct Saw {
    ictx: format::context::Input,
    pub start: f64,
    pub first_kf: Option<f64>,
    pub last_kf: Option<f64>,
    end: f64,
}

impl std::fmt::Debug for Saw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Saw")
            .field("ictx", &"InputContext { .. }") // or just omit this field
            .field("start", &self.start)
            .field("first_kf", &self.first_kf)
            .field("last_kf", &self.last_kf)
            .field("end", &self.end)
            .finish()
    }
}

impl Saw {
    pub fn new(input: &str, start: f64, end: f64) -> Result<Saw, Error> {
        ffmpeg::init()?;
        let ictx = format::input(&input)?;
        Ok(Saw {
            ictx,
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
