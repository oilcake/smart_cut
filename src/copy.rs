use failure::Error;

pub fn remux_with_seek(
    input: &str,
    output: &str,
    start_time: f64,
    end_time: f64,
) -> Result<(), Error> {
    ffmpeg_next::init()?;
    ffmpeg_next::util::log::set_level(ffmpeg_next::util::log::Level::Debug);

    let mut ictx = ffmpeg_next::format::input(&input)?;
    let mut octx = ffmpeg_next::format::output(&output)?;

    // Создаем маппинг всех потоков (включая видео)
    let mut stream_map = std::collections::HashMap::new();
    for istream in ictx.streams() {
        let idx = istream.index();
        let mut ostream = octx.add_stream(istream.parameters().id())?;
        ostream.set_parameters(istream.parameters());
        stream_map.insert(idx, ostream.index());
    }

    // Получаем видео поток для seek
    let video_stream_index = ictx
        .streams()
        .best(ffmpeg_next::media::Type::Video)
        .ok_or(ffmpeg_next::Error::StreamNotFound)?
        .index();

    let video_stream = ictx.stream(video_stream_index).ok_or(ffmpeg_next::Error::StreamNotFound)?;
    let time_base = video_stream.time_base();
    let seek_ts = (start_time / av_q2d(time_base)) as i64;
    let end_ts = (end_time / av_q2d(time_base)) as i64;

    // Выполняем seek
    unsafe {
        ffmpeg_next::ffi::av_seek_frame(
            ictx.as_mut_ptr(),
            video_stream_index as i32,
            seek_ts,
            ffmpeg_next::ffi::AVSEEK_FLAG_BACKWARD,
        );
    }

    octx.write_header()?;

    // Обрабатываем пакеты
    for (istream, mut packet) in ictx.packets() {
        if let Some(&out_idx) = stream_map.get(&istream.index()) {
            let ostream = octx.stream(out_idx).unwrap();
            
            // Рескалируем временные метки
            packet.rescale_ts(istream.time_base(), ostream.time_base());
            
            // Проверяем временные метки для видео
            if istream.index() == video_stream_index {
                if let Some(pts) = packet.pts() {
                    if pts > end_ts {
                        break;
                    }
                }
            }
            
            packet.set_stream(out_idx);
            packet.write_interleaved(&mut octx)?;
        }
    }

    octx.write_trailer()?;
    Ok(())
}

// Вспомогательная функция для конвертации AVRational в f64
fn av_q2d(r: ffmpeg_next::util::rational::Rational) -> f64 {
    r.numerator() as f64 / r.denominator() as f64
}
