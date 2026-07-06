use crate::planner_pipeline::Plan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepChange {
    Added(String),
    Removed(String),
    Retained(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDiff {
    pub old_id: String,
    pub new_id: String,
    pub changes: Vec<StepChange>,
}

impl PlanDiff {
    pub fn diff(old: &Plan, new: &Plan) -> Self {
        let a: Vec<&str> = old.steps.iter().map(String::as_str).collect();
        let b: Vec<&str> = new.steps.iter().map(String::as_str).collect();
        let lcs = lcs_table(&a, &b);
        let changes = backtrack(&lcs, &a, &b, a.len(), b.len());
        Self { old_id: old.id.clone(), new_id: new.id.clone(), changes }
    }

    pub fn is_identical(&self) -> bool {
        self.changes.iter().all(|c| matches!(c, StepChange::Retained(_)))
    }

    pub fn added(&self) -> Vec<&str> {
        self.changes.iter().filter_map(|c| {
            if let StepChange::Added(s) = c { Some(s.as_str()) } else { None }
        }).collect()
    }

    pub fn removed(&self) -> Vec<&str> {
        self.changes.iter().filter_map(|c| {
            if let StepChange::Removed(s) = c { Some(s.as_str()) } else { None }
        }).collect()
    }
}

fn lcs_table(a: &[&str], b: &[&str]) -> Vec<Vec<usize>> {
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if a[i-1] == b[j-1] { dp[i-1][j-1] + 1 }
                       else { dp[i-1][j].max(dp[i][j-1]) };
        }
    }
    dp
}

fn backtrack(dp: &[Vec<usize>], a: &[&str], b: &[&str], i: usize, j: usize) -> Vec<StepChange> {
    if i == 0 && j == 0 { return vec![]; }
    if i == 0 {
        let mut v = backtrack(dp, a, b, i, j-1);
        v.push(StepChange::Added(b[j-1].to_owned())); return v;
    }
    if j == 0 {
        let mut v = backtrack(dp, a, b, i-1, j);
        v.push(StepChange::Removed(a[i-1].to_owned())); return v;
    }
    if a[i-1] == b[j-1] {
        let mut v = backtrack(dp, a, b, i-1, j-1);
        v.push(StepChange::Retained(a[i-1].to_owned())); v
    } else if dp[i-1][j] >= dp[i][j-1] {
        let mut v = backtrack(dp, a, b, i-1, j);
        v.push(StepChange::Removed(a[i-1].to_owned())); v
    } else {
        let mut v = backtrack(dp, a, b, i, j-1);
        v.push(StepChange::Added(b[j-1].to_owned())); v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner_pipeline::Plan;

    fn plan(_id: &str, steps: &[&str]) -> Plan {
        Plan::new_with_stable_id(0, steps.iter().map(|s| s.to_string()).collect())
    }

    fn plan_with_id(id: &str, steps: &[&str]) -> Plan {
        let mut p = plan(id, steps);
        p.id = id.to_owned();
        p
    }

    #[test]
    fn diff_identical_plans_is_all_retained() {
        let a = plan("a", &["step one", "step two"]);
        let diff = PlanDiff::diff(&a, &a);
        assert!(diff.is_identical());
        assert!(diff.added().is_empty());
        assert!(diff.removed().is_empty());
    }

    #[test]
    fn diff_detects_added_step() {
        let old = plan("old", &["step one"]);
        let new = plan("new", &["step one", "step two"]);
        let diff = PlanDiff::diff(&old, &new);
        assert!(!diff.is_identical());
        assert_eq!(diff.added(), vec!["step two"]);
        assert!(diff.removed().is_empty());
    }

    #[test]
    fn diff_detects_removed_step() {
        let old = plan("old", &["step one", "step two"]);
        let new = plan("new", &["step one"]);
        let diff = PlanDiff::diff(&old, &new);
        assert!(!diff.is_identical());
        assert!(diff.added().is_empty());
        assert_eq!(diff.removed(), vec!["step two"]);
    }

    #[test]
    fn diff_ids_match_plans() {
        let old = plan_with_id("id-old", &["a"]);
        let new = plan_with_id("id-new", &["b"]);
        let diff = PlanDiff::diff(&old, &new);
        assert_eq!(diff.old_id, "id-old");
        assert_eq!(diff.new_id, "id-new");
    }

    #[test]
    fn diff_empty_to_nonempty() {
        let old = plan("e", &[]);
        let new = plan("f", &["x", "y"]);
        let diff = PlanDiff::diff(&old, &new);
        assert_eq!(diff.added().len(), 2);
        assert!(diff.removed().is_empty());
    }
}
