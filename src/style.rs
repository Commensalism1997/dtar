const DPROGTEMP: &str = "{msg:.7}  [{elapsed_precise:.8}] {wide_bar:.4/8} {human_pos}/{human_len} {percent}%  [{eta:.7}]";
const DPROGCHARS: &str = "▒█░";

pub fn themed_progressbar(len: u64) -> indicatif::ProgressBar {
    indicatif::ProgressBar::new(len).with_style(indicatif::ProgressStyle::with_template(DPROGTEMP).unwrap().progress_chars(DPROGCHARS))
}

pub fn themed_spinner() -> indicatif::ProgressBar {
    indicatif::ProgressBar::new_spinner()
}