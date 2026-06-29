// const DPROGTEMP: &str = "{msg:.7}  [{elapsed_precise:.8}] {wide_bar:.4/8} {human_pos}/{human_len} {percent}%  [{eta:.7}]";
const DPROGTEMPBYTESBLUE: &str = "{msg:.7}  [{elapsed_precise:.8}] {wide_bar:.4/8} {bytes}/{total_bytes} {percent}%  [{eta:.7}]";
const DPROGTEMPBYTESGREEN: &str = "{msg:.7}  [{elapsed_precise:.8}] {wide_bar:.2/8} {bytes}/{total_bytes} {percent}%  [{eta:.7}]";
const DPROGCHARS: &str = "▒█░";

// pub fn themed_progressbar(len: u64) -> indicatif::ProgressBar {
//     indicatif::ProgressBar::new(len).with_style(indicatif::ProgressStyle::with_template(DPROGTEMP).unwrap().progress_chars(DPROGCHARS))
// }

pub fn themed_progressbar_bytes_blue(len: u64) -> indicatif::ProgressBar {
    indicatif::ProgressBar::new(len).with_style(indicatif::ProgressStyle::with_template(DPROGTEMPBYTESBLUE).unwrap().progress_chars(DPROGCHARS))
}

pub fn themed_progressbar_bytes_green(len: u64) -> indicatif::ProgressBar {
    indicatif::ProgressBar::new(len).with_style(indicatif::ProgressStyle::with_template(DPROGTEMPBYTESGREEN).unwrap().progress_chars(DPROGCHARS))
}

pub fn themed_spinner() -> indicatif::ProgressBar {
    indicatif::ProgressBar::new_spinner()
}