use console::{Attribute, Style};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::time::Duration;
use tokio::time::Instant;

use crate::job::job_processor::{JobProgress, JobResult};

pub struct ProgressDisplay {
    mpb: MultiProgress,
    status_line: ProgressBar,
    pb_total: ProgressBar,

    num_valid: u64,
    num_infeasible: u64,
    num_emptysolution: u64,
    num_invalidinstance: u64,
    num_syntaxerror: u64,
    num_systemerror: u64,
    num_solvererror: u64,
    num_timeout: u64,
}

impl ProgressDisplay {
    pub fn new(num_instances: usize) -> Self {
        let mpb = MultiProgress::new();

        let status_line = mpb.add(ProgressBar::no_length());
        status_line.set_style(ProgressStyle::default_bar().template("{msg}").unwrap());

        let pb_total = mpb.add(indicatif::ProgressBar::new(num_instances as u64));
        pb_total.set_style(
            ProgressStyle::with_template("{msg:<15} [{elapsed_precise}] [{bar:50.green/grey}] {human_pos} of {human_len} (est: {eta})").unwrap()
                .progress_chars("#>-"),
        );

        pb_total.set_message("Total finished");

        Self {
            mpb,
            status_line,
            pb_total,
            num_valid: 0,
            num_infeasible: 0,
            num_invalidinstance: 0,
            num_syntaxerror: 0,
            num_systemerror: 0,
            num_solvererror: 0,
            num_timeout: 0,
            num_emptysolution: 0,
        }
    }

    fn multi_progress(&self) -> &MultiProgress {
        &self.mpb
    }

    pub fn tick(&mut self, running: usize) {
        macro_rules! format_num {
            ($key:ident, $name:expr, $color:ident) => {
                format_num!($key, $name, $color, [])
            };
            ($key:ident, $name:expr, $color:ident, $attrs : expr) => {{
                let text = format!("{}: {:>6}", $name, self.$key);
                if self.$key == 0 {
                    text
                } else {
                    let mut style = console::Style::new().$color();
                    for x in $attrs {
                        style = style.attr(x);
                    }

                    style.apply_to(text).to_string()
                }
            }};
        }

        const CRITICAL: [Attribute; 2] = [Attribute::Bold, Attribute::Underlined];
        let parts = [
            format_num!(num_valid, "Valid", green),
            format_num!(num_emptysolution, "Empty", yellow),
            format_num!(num_infeasible, "Infeas", yellow, CRITICAL),
            format_num!(num_syntaxerror, "SyntErr", red),
            format_num!(num_solvererror, "SolvErr", red),
            format_num!(num_systemerror, "SysErr", red),
            format!("Running: {running}"),
        ];

        self.status_line.set_message(parts.join(" | "));
    }

    pub fn finish_job(&mut self, result: JobResult) {
        self.pb_total.inc(1);

        match result {
            JobResult::Valid { .. } => self.num_valid += 1,
            JobResult::Infeasible => self.num_infeasible += 1,
            JobResult::InvalidInstance => self.num_invalidinstance += 1,
            JobResult::SyntaxError => self.num_syntaxerror += 1,
            JobResult::SystemError => self.num_systemerror += 1,
            JobResult::SolverError => self.num_solvererror += 1,
            JobResult::Timeout => self.num_timeout += 1,
            JobResult::EmptySolution => self.num_emptysolution += 1,
        }
    }

    pub fn final_message(&self) {
        println!("{}", self.status_line.message());
    }
}

pub struct JobProgressBar {
    pb: Option<ProgressBar>,
    instance_name: String,

    soft_timeout: Duration,

    previous_progress: Option<JobProgress>,
    start: Instant,
    max_time_millis: u64,
}

impl JobProgressBar {
    const MILLIS_BEFORE_PROGRESS_BAR: u64 = 100;
    const MAX_INSTANCE_NAME_LENGTH: usize = 14;

    pub fn new(mut instance_name: String, soft_timeout: Duration, grace_period: Duration) -> Self {
        let max_time_millis = (soft_timeout + grace_period).as_millis() as u64;

        if let Some((idx, _)) = instance_name
            .char_indices()
            .nth(Self::MAX_INSTANCE_NAME_LENGTH)
            && idx < instance_name.len()
        {
            instance_name.truncate(idx);
        }

        Self {
            start: Instant::now(),
            instance_name,
            max_time_millis,
            pb: None,
            previous_progress: None,
            soft_timeout,
        }
    }

    pub fn update_progress_bar(
        &mut self,
        mpb: &ProgressDisplay,
        progress: JobProgress,
        now: Instant,
    ) {
        let elapsed = (now.duration_since(self.start).as_millis() as u64).min(self.max_time_millis);
        if elapsed < Self::MILLIS_BEFORE_PROGRESS_BAR {
            return; // do not create a progress bar for short running tasks
        }

        if self.pb.is_none() {
            self.create_pb(mpb.multi_progress());
        }

        let pb = self.pb.as_ref().unwrap();

        if Some(progress) != self.previous_progress {
            self.previous_progress = Some(progress);
            self.start = now;
            self.pb.as_ref().unwrap().reset_elapsed();

            match progress {
                JobProgress::Running => self.style_for_running(pb),
                JobProgress::Checking => self.style_for_waiting(pb),
                JobProgress::Finished | JobProgress::Starting => {}
            }
        }

        let message: String = match progress {
            JobProgress::Starting => "starting".into(),
            JobProgress::Running => {
                if elapsed > self.soft_timeout.as_millis() as u64 {
                    Style::new().red().apply_to("grace").to_string()
                } else {
                    "running".into()
                }
            }
            JobProgress::Checking => "checking".into(),
            JobProgress::Finished => "done".into(),
        };

        pb.set_message(message);
        pb.set_position(elapsed);
    }

    pub fn finish(&self, display: &mut ProgressDisplay, result: JobResult) {
        if let Some(pb) = &self.pb {
            display.multi_progress().remove(pb);
        }

        display.finish_job(result);
    }

    fn create_pb(&mut self, mpb: &MultiProgress) {
        let pb = mpb.add(ProgressBar::new(self.max_time_millis));
        self.pb = Some(pb);
    }

    fn style_for_running(&self, pb: &ProgressBar) {
        let mut template = format!("{: <15} ", self.instance_name);
        template += "[{elapsed_precise}] [{bar:50.cyan/blue}] {msg}";

        pb.set_style(
            ProgressStyle::default_bar()
                .template(&template)
                .unwrap()
                .progress_chars("#>-"),
        );

        pb.set_length(self.max_time_millis);
    }

    fn style_for_waiting(&self, pb: &ProgressBar) {
        let mut template = format!("{: <15} ", self.instance_name);
        template += "[{elapsed_precise}] {spinner:.green}                                                    {msg}";

        pb.set_style(ProgressStyle::default_bar().template(&template).unwrap());
    }
}
