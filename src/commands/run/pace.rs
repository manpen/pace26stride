pub fn compute_pace_heuristic_score(
    solver_score: usize,
    best_known: usize,
    num_leaves: usize,
) -> f64 {
    let best_known = best_known.min(solver_score);
    let upper = num_leaves.min(2 * best_known);

    ((upper - solver_score) as f64 / (upper - best_known) as f64)
        .max(0.0)
        .powi(2)
}

#[cfg(test)]
mod test {
    use crate::commands::run::pace::compute_pace_heuristic_score;

    #[test]
    fn heuristic_score() {
        assert_eq!(compute_pace_heuristic_score(2, 2, 5), 1.0);
        assert_eq!(compute_pace_heuristic_score(2, 3, 5), 1.0);
        assert_eq!(compute_pace_heuristic_score(5, 3, 5), 0.0);
        assert_eq!(compute_pace_heuristic_score(4, 2, 5), 0.0);
        assert!((0.1..0.9).contains(&compute_pace_heuristic_score(3, 2, 5)));
    }
}
