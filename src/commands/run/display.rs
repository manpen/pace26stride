use console::{Attribute, Style};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::time::Instant;

use crate::job::job_processor::{JobProgress, JobResult};

pub struct ProgressDisplay {
    mpb: MultiProgress,
    status_line: ProgressBar,
    stride_line: ProgressBar,
    pb_total: ProgressBar,

    num_valid: AtomicU64,
    num_infeasible: AtomicU64,
    num_emptysolution: AtomicU64,
    num_invalidinstance: AtomicU64,
    num_syntaxerror: AtomicU64,
    num_systemerror: AtomicU64,
    num_solvererror: AtomicU64,
    num_timeout: AtomicU64,

    num_stride_instances: AtomicU64,
    num_stride_queued: AtomicU64,
    num_stride_best_known: AtomicU64,
    num_stride_new_best_known: AtomicU64,
    num_stride_no_response: AtomicU64,
    num_stride_suboptimal: AtomicU64,
}

impl ProgressDisplay {
    pub fn new(num_instances: usize) -> Self {
        let mpb = MultiProgress::new();

        let status_line = mpb.add(ProgressBar::no_length());
        status_line.set_style(ProgressStyle::default_bar().template("{msg}").unwrap());

        let stride_line = ProgressBar::no_length();
        stride_line.set_style(ProgressStyle::default_bar().template("{msg}").unwrap());

        let pb_total = mpb.add(ProgressBar::new(num_instances as u64));
        pb_total.set_style(
            ProgressStyle::with_template("{msg:<15.cyan} [{elapsed_precise:.cyan}] [{bar:60.cyan/grey}] {human_pos.cyan} of {human_len} (est: {eta})").unwrap()
                .progress_chars("#>-"),
        );

        pb_total.set_message("Completed tasks     ");

        Self {
            mpb,
            status_line,
            pb_total,
            stride_line,

            num_valid: Default::default(),
            num_infeasible: Default::default(),
            num_invalidinstance: Default::default(),
            num_syntaxerror: Default::default(),
            num_systemerror: Default::default(),
            num_solvererror: Default::default(),
            num_timeout: Default::default(),
            num_emptysolution: Default::default(),

            num_stride_instances: Default::default(),
            num_stride_queued: Default::default(),
            num_stride_best_known: Default::default(),
            num_stride_new_best_known: Default::default(),
            num_stride_no_response: Default::default(),
            num_stride_suboptimal: Default::default(),
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
        self.tick(0);
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

                let name = $name;
                let name_wo_space = name.trim_end();
                let space = &name[name_wo_space.len()..];

                let text = format!("{name_wo_space}:{space} {value:>6}");
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
        {
            let parts = [
                format_num!(num_valid, "Valid", green),
                format_num!(num_emptysolution, "Empty   ", yellow),
                format_num!(num_infeasible, "Infeas", yellow, CRITICAL),
                format_num!(num_syntaxerror, "SyntErr", red),
                format_num!(num_solvererror, "SolvErr ", red),
                format_num!(num_systemerror, "SysErr", red),
                format!("Running: {running}"),
            ];

            self.status_line.set_message(parts.join(" | "));
        }

        if self.num_stride_instances.load(Ordering::Acquire) == 0 {
            return;
        }

        {
            let parts = [
                format_num!(num_stride_best_known, "Best ", green),
                format_num!(num_stride_new_best_known, "New Best", yellow),
                format_num!(num_stride_suboptimal, "Subopt", red, CRITICAL),
                format_num!(num_stride_no_response, "No Resp", yellow),
                format_num!(num_stride_queued, "Transmit", green),
                format_num!(num_stride_instances, "STRIDE Instances", white),
            ];

            self.stride_line.set_message(parts.join(" | "));
        }
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

    /////////////// STRIDE
    pub fn set_num_stride_instance(&self, num_instances: usize) {
        let prev = self
            .num_stride_instances
            .fetch_add(num_instances as u64, Ordering::Release);
        assert_eq!(prev, 0);
        if num_instances > 0 {
            self.mpb
                .insert_after(&self.status_line, self.stride_line.clone());
        }
    }

    pub fn stride_inc_queued(&self) {
        self.num_stride_queued.fetch_add(1, Ordering::AcqRel);
    }

    pub fn stride_inc_best_known(&self) {
        self.num_stride_queued.fetch_sub(1, Ordering::AcqRel);
        self.num_stride_best_known.fetch_add(1, Ordering::AcqRel);
    }

    pub fn stride_new_best_known(&self) {
        self.num_stride_queued.fetch_sub(1, Ordering::AcqRel);
        self.num_stride_new_best_known
            .fetch_add(1, Ordering::AcqRel);
        self.stride_inc_best_known();
    }

    pub fn stride_inc_no_response(&self) {
        self.num_stride_queued.fetch_sub(1, Ordering::AcqRel);
        self.num_stride_no_response.fetch_add(1, Ordering::AcqRel);
    }

    pub fn stride_suboptimal(&self) {
        self.num_stride_queued.fetch_sub(1, Ordering::AcqRel);
        self.num_stride_suboptimal.fetch_add(1, Ordering::AcqRel);
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
    const MAX_INSTANCE_NAME_LENGTH: usize = 20;

    pub fn new(mut instance_name: String, soft_timeout: Duration, grace_period: Duration) -> Self {
        let max_time_millis = (soft_timeout + grace_period).as_millis() as u64;

        if let Some((idx, _)) = instance_name
            .char_indices()
            .nth(Self::MAX_INSTANCE_NAME_LENGTH)
            && idx < instance_name.len()
        {
            instance_name.truncate(idx);
        }

        while instance_name.len() < Self::MAX_INSTANCE_NAME_LENGTH {
            instance_name.push(' ');
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
        template += "[{elapsed_precise}] [{bar:60.cyan/blue}] {msg}";

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
