use std::process::Command;
use std::path::Path;
use std::io::{self, Error, ErrorKind};

/// Копирует фрагмент видео без перекодирования, используя системный ffmpeg
/// 
/// # Аргументы
/// * `input_path` - путь к исходному файлу
/// * `output_path` - путь для сохранения результата
/// * `start_time` - начальное время в секундах (f64)
/// * `duration` - длительность фрагмента в секундах (f64)
/// 
/// # Пример
/// ```
/// copy_video_fragment(
///     "input.mp4",
///     "output.mp4",
///     10.5,  // начинаем с 10.5 секунды
///     30.0   // длительность 30 секунд
/// ).unwrap();
/// ```
pub fn copy_video_fragment(
    input_path: &str,
    output_path: &str,
    start_time: f64,
    duration: f64,
) -> io::Result<()> {
    // Проверяем существование входного файла
    if !Path::new(input_path).exists() {
        return Err(Error::new(
            ErrorKind::NotFound,
            format!("Input file not found: {}", input_path),
        ));
    }

    // Формируем команду ffmpeg
    let status = Command::new("ffmpeg")
        .args(&[
            "-ss",
            &format!("{}", start_time),  // точность до миллисекунд
            "-i",
            input_path,
            "-t",
            &format!("{}", duration),
            "-c",
            "copy",  // копируем без перекодирования
            "-avoid_negative_ts",
            "make_zero",
            "-y",     // перезаписываем выходной файл без подтверждения
            output_path,
        ])
        .status()?;

    if !status.success() {
        return Err(Error::new(
            ErrorKind::Other,
            format!("FFmpeg failed with exit code: {:?}", status.code()),
        ));
    }

    Ok(())
}
