use console::{Attribute, Style};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::time::Instant;

use crate::job::job_processor::{JobProgress, JobResult};

pub struct ProgressDisplay {
    mpb: MultiProgress,
    status_line: ProgressBar,
    pb_total: ProgressBar,

    num_valid: AtomicU64,
    num_infeasible: AtomicU64,
    num_emptysolution: AtomicU64,
    num_invalidinstance: AtomicU64,
    num_syntaxerror: AtomicU64,
    num_systemerror: AtomicU64,
    num_solvererror: AtomicU64,
    num_timeout: AtomicU64,
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
            num_valid: Default::default(),
            num_infeasible: Default::default(),
            num_invalidinstance: Default::default(),
            num_syntaxerror: Default::default(),
            num_systemerror: Default::default(),
            num_solvererror: Default::default(),
            num_timeout: Default::default(),
            num_emptysolution: Default::default(),
        }
    }

    pub fn set_total_instance(&self, num_instances: usize) {
        self.pb_total.set_length(num_instances as u64);
    }

    fn multi_progress(&self) -> &MultiProgress {
        &self.mpb
    }

    pub fn switch_to_postprocessing(&self) {
        self.pb_total.set_length(100000000);
        self.pb_total.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} {msg}")
                .unwrap()
                .progress_chars("#>-"),
        );
        self.pb_total
            .set_message("Postprocessing ... this may take a few seconds");
    }

    pub fn post_processing_tick(&self) {
        self.pb_total.inc(1);
        self.pb_total.tick();
    }

    pub fn tick(&self, running: usize) {
        macro_rules! format_num {
            ($key:ident, $name:expr, $color:ident) => {
                format_num!($key, $name, $color, [])
            };
            ($key:ident, $name:expr, $color:ident, $attrs : expr) => {{
                let value = self.$key.load(Ordering::Acquire);
                let text = format!("{}: {value:>6}", $name);
                if value == 0 {
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

    pub fn finish_job(&self, result: JobResult) {
        self.pb_total.inc(1);

        match result {
            JobResult::Valid { .. } => {
                self.num_valid.fetch_add(1, Ordering::AcqRel);
            }
            JobResult::Infeasible => {
                self.num_infeasible.fetch_add(1, Ordering::AcqRel);
            }
            JobResult::InvalidInstance => {
                self.num_invalidinstance.fetch_add(1, Ordering::AcqRel);
            }
            JobResult::SyntaxError => {
                self.num_syntaxerror.fetch_add(1, Ordering::AcqRel);
            }
            JobResult::SystemError => {
                self.num_systemerror.fetch_add(1, Ordering::AcqRel);
            }
            JobResult::SolverError => {
                self.num_solvererror.fetch_add(1, Ordering::AcqRel);
            }
            JobResult::Timeout => {
                self.num_timeout.fetch_add(1, Ordering::AcqRel);
            }
            JobResult::EmptySolution => {
                self.num_emptysolution.fetch_add(1, Ordering::AcqRel);
            }
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

    pub fn update_progress_bar(&mut self, mpb: &ProgressDisplay, progress: JobProgress) {
        let now = Instant::now();
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

    pub fn finish(&self, display: &ProgressDisplay, result: JobResult) {
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
