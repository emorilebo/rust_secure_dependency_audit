//! Health scoring algorithms for dependencies
//!
//! Each component score is `Some(0..=100)` when it could be measured and
//! `None` when its inputs are missing. The overall score is the weighted
//! average of the measured components, with the weights renormalised over
//! what was measured — the same way OpenSSF Scorecard leaves inconclusive
//! checks out of its aggregate. A dependency with no crates.io and no
//! repository data is not scored at all (`HealthStatus::Unknown`).

use crate::config::AuditConfig;
use crate::metadata::{CrateMetadata, GitHubMetadata, GitLabMetadata};
use crate::types::{ComponentScores, DependencyMetrics, HealthStatus, RepositoryMetrics};
use chrono::{DateTime, Utc};

/// Inputs available for one dependency.
#[derive(Debug, Default, Clone, Copy)]
pub struct ScoringInputs<'a> {
    pub crate_meta: Option<&'a CrateMetadata>,
    pub github: Option<&'a GitHubMetadata>,
    pub gitlab: Option<&'a GitLabMetadata>,
    pub openssf_score: Option<f32>,
}

impl ScoringInputs<'_> {
    fn has_repository_data(&self) -> bool {
        self.github.is_some() || self.gitlab.is_some()
    }

    /// Whether there is enough data to score at all.
    pub fn is_scorable(&self) -> bool {
        self.crate_meta.is_some() || self.has_repository_data()
    }
}

/// Calculate overall health score for a dependency.
///
/// Returns `None` when nothing could be measured.
pub fn calculate_health_score(
    inputs: ScoringInputs<'_>,
    config: &AuditConfig,
) -> Option<(u8, DependencyMetrics)> {
    if !inputs.is_scorable() {
        return None;
    }

    let weights = &config.scoring_weights;
    let scores = ComponentScores {
        recency: calculate_recency_score(&inputs, config),
        maintenance: calculate_maintenance_score(&inputs),
        community: calculate_community_score(&inputs),
        stability: calculate_stability_score(&inputs),
        security: calculate_security_score(&inputs),
    };

    let weighted = [
        (scores.recency, weights.recency),
        (scores.maintenance, weights.maintenance),
        (scores.community, weights.community),
        (scores.stability, weights.stability),
        (scores.security, weights.security),
    ];

    let (sum, weight_sum) = weighted
        .iter()
        .filter_map(|(score, weight)| score.map(|s| (s * weight, *weight)))
        .fold((0.0_f32, 0.0_f32), |(sum, total), (ws, w)| {
            (sum + ws, total + w)
        });

    if weight_sum <= f32::EPSILON {
        return None;
    }

    let mut overall = (sum / weight_sum).round();

    // Penalize yanked crates heavily
    if inputs.crate_meta.is_some_and(|meta| meta.is_yanked) {
        overall = (overall * 0.1).min(10.0); // Max score 10 for yanked crates
    }

    let overall = overall.clamp(0.0, 100.0) as u8;
    Some((overall, build_metrics(&inputs, scores)))
}

/// Determine health status from score
pub fn determine_status(score: u8, _config: &AuditConfig) -> HealthStatus {
    if score >= 80 {
        HealthStatus::Healthy
    } else if score >= 60 {
        HealthStatus::Warning
    } else if score >= 40 {
        HealthStatus::Stale
    } else {
        HealthStatus::Risky
    }
}

fn days_since(when: DateTime<Utc>) -> u32 {
    Utc::now()
        .signed_duration_since(when)
        .num_days()
        .clamp(0, i64::from(u32::MAX)) as u32
}

/// Most recent activity: repository push if known, else newest crates.io release.
fn last_activity(inputs: &ScoringInputs<'_>) -> Option<DateTime<Utc>> {
    inputs
        .github
        .map(|gh| gh.pushed_at)
        .or_else(|| inputs.gitlab.map(|gl| gl.last_activity_at))
        .or_else(|| inputs.crate_meta.map(|cr| cr.last_published))
}

/// Calculate recency score based on last update
fn calculate_recency_score(inputs: &ScoringInputs<'_>, config: &AuditConfig) -> Option<f32> {
    let days_old = days_since(last_activity(inputs)?);

    let stale_days = config.staleness_thresholds.stale_days;
    let risky_days = config.staleness_thresholds.risky_days;

    Some(if days_old <= 30 {
        100.0 // Updated within last month
    } else if days_old <= 90 {
        90.0 // Updated within last quarter
    } else if days_old <= 180 {
        80.0 // Updated within 6 months
    } else if days_old <= stale_days {
        60.0 // Getting old but not stale yet
    } else if days_old <= risky_days {
        30.0 // Stale
    } else {
        10.0 // Very stale/risky
    })
}

/// Calculate maintenance score from repository activity
fn calculate_maintenance_score(inputs: &ScoringInputs<'_>) -> Option<f32> {
    let (archived, open_issues, last_activity) = match (inputs.github, inputs.gitlab) {
        (Some(gh), _) => (gh.is_archived, gh.open_issues, gh.pushed_at),
        (None, Some(gl)) => (gl.is_archived, gl.open_issues, gl.last_activity_at),
        (None, None) => return None,
    };

    // Archived repo is a major red flag
    if archived {
        return Some(0.0);
    }

    let mut score: f32 = 50.0;

    // Low open issues is good
    if open_issues < 10 {
        score += 25.0;
    } else if open_issues < 50 {
        score += 10.0;
    } else if open_issues > 200 {
        score -= 10.0;
    }

    // Recent activity is good
    let days_since_activity = days_since(last_activity);
    if days_since_activity <= 30 {
        score += 25.0;
    } else if days_since_activity <= 90 {
        score += 15.0;
    } else if days_since_activity > 365 {
        score -= 20.0;
    }

    Some(score.clamp(0.0, 100.0))
}

/// Calculate community score from publishers, stars and contributors.
///
/// Stars and contributors are the main inputs, so this component needs
/// repository data; the crates.io publisher count is added on top.
fn calculate_community_score(inputs: &ScoringInputs<'_>) -> Option<f32> {
    let stars = inputs
        .github
        .map(|gh| gh.stars)
        .or_else(|| inputs.gitlab.map(|gl| gl.stars))?;

    let mut score: f32 = 0.0;

    if let Some(crate_meta) = inputs.crate_meta {
        score += match crate_meta.publisher_count {
            0 => 0.0,
            1 => 30.0,
            2..=5 => 50.0,
            6..=10 => 70.0,
            _ => 80.0,
        };
    }

    score += match stars {
        0..=10 => 0.0,
        11..=50 => 10.0,
        51..=200 => 20.0,
        201..=1000 => 30.0,
        _ => 40.0,
    };

    if let Some(contributors) = inputs.github.and_then(|gh| gh.contributors_count) {
        score += match contributors {
            0..=1 => 0.0,
            2..=5 => 10.0,
            6..=20 => 20.0,
            _ => 30.0,
        };
    }

    Some(score.clamp(0.0, 100.0))
}

/// Calculate stability score from version history
fn calculate_stability_score(inputs: &ScoringInputs<'_>) -> Option<f32> {
    let meta = inputs.crate_meta?;

    // More versions generally indicates active maintenance
    let score: f32 = match meta.version_count {
        0..=1 => 20.0,
        2..=5 => 40.0,
        6..=10 => 60.0,
        11..=30 => 80.0,
        _ => 100.0,
    };

    // Bonus for high download count (indicates trust)
    let download_bonus = if meta.downloads > 1_000_000 {
        10.0
    } else if meta.downloads > 100_000 {
        5.0
    } else {
        0.0
    };

    Some((score + download_bonus).clamp(0.0, 100.0))
}

/// Calculate security score based on OpenSSF Scorecard or SECURITY.md
fn calculate_security_score(inputs: &ScoringInputs<'_>) -> Option<f32> {
    if inputs.crate_meta.is_some_and(|meta| meta.is_yanked) {
        return Some(0.0);
    }

    // OpenSSF Scorecard (0-10) -> 0-100
    if let Some(ossf) = inputs.openssf_score {
        return Some((ossf * 10.0).clamp(0.0, 100.0));
    }

    // Fallback heuristic: presence of a security policy
    inputs
        .github
        .and_then(|gh| gh.has_security_policy)
        .map(|has_policy| if has_policy { 70.0 } else { 40.0 })
}

/// Build detailed metrics object
fn build_metrics(inputs: &ScoringInputs<'_>, scores: ComponentScores) -> DependencyMetrics {
    let repository = inputs
        .github
        .map(|gh| RepositoryMetrics {
            open_issues: Some(gh.open_issues),
            contributor_count: gh.contributors_count,
            days_since_last_commit: Some(days_since(gh.pushed_at)),
            stars: Some(gh.stars),
            is_archived: Some(gh.is_archived),
            has_security_policy: gh.has_security_policy,
        })
        .or_else(|| {
            inputs.gitlab.map(|gl| RepositoryMetrics {
                open_issues: Some(gl.open_issues),
                contributor_count: None,
                days_since_last_commit: Some(days_since(gl.last_activity_at)),
                stars: Some(gl.stars),
                is_archived: Some(gl.is_archived),
                has_security_policy: None,
            })
        });

    DependencyMetrics {
        days_since_last_update: last_activity(inputs).map(days_since),
        version_count: inputs.crate_meta.map(|m| m.version_count),
        maintainer_count: inputs.crate_meta.map(|m| m.publisher_count),
        repository,
        openssf_score: inputs.openssf_score,
        scores,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn crate_meta(days_since_publish: i64) -> CrateMetadata {
        CrateMetadata {
            license: None,
            repository: None,
            downloads: 5_000_000,
            last_published: Utc::now() - Duration::days(days_since_publish),
            version_count: 40,
            publisher_count: 1,
            is_yanked: false,
        }
    }

    fn github(days_since_push: i64) -> GitHubMetadata {
        GitHubMetadata {
            stars: 5_000,
            open_issues: 5,
            is_archived: false,
            pushed_at: Utc::now() - Duration::days(days_since_push),
            contributors_count: Some(150),
            has_security_policy: Some(true),
        }
    }

    #[test]
    fn test_recency_score_recent() {
        let config = AuditConfig::default();
        let meta = crate_meta(15);
        let inputs = ScoringInputs {
            crate_meta: Some(&meta),
            ..ScoringInputs::default()
        };
        let score = calculate_recency_score(&inputs, &config).unwrap();
        assert!(score >= 90.0, "Recent update should score high");
    }

    #[test]
    fn test_determine_status() {
        let config = AuditConfig::default();

        assert_eq!(determine_status(85, &config), HealthStatus::Healthy);
        assert_eq!(determine_status(65, &config), HealthStatus::Warning);
        assert_eq!(determine_status(45, &config), HealthStatus::Stale);
        assert_eq!(determine_status(25, &config), HealthStatus::Risky);
    }

    #[test]
    fn nothing_to_score_is_unknown() {
        let config = AuditConfig::default();
        assert!(calculate_health_score(ScoringInputs::default(), &config).is_none());

        // An OpenSSF score alone is not enough to judge maintenance health.
        let inputs = ScoringInputs {
            openssf_score: Some(7.0),
            ..ScoringInputs::default()
        };
        assert!(calculate_health_score(inputs, &config).is_none());
    }

    #[test]
    fn missing_repository_data_is_left_out_not_counted_as_zero() {
        // Regression: when GitHub was rate limited, a healthy crate lost its
        // maintenance/community signals and was scored as if they were bad.
        let config = AuditConfig::default();
        let meta = crate_meta(10);
        let gh = github(10);

        let with_repo = ScoringInputs {
            crate_meta: Some(&meta),
            github: Some(&gh),
            openssf_score: Some(8.0),
            ..ScoringInputs::default()
        };
        let without_repo = ScoringInputs {
            github: None,
            ..with_repo
        };

        let (full, full_metrics) = calculate_health_score(with_repo, &config).unwrap();
        let (partial, partial_metrics) = calculate_health_score(without_repo, &config).unwrap();

        assert_eq!(determine_status(full, &config), HealthStatus::Healthy);
        assert_eq!(determine_status(partial, &config), HealthStatus::Healthy);
        assert_eq!(partial_metrics.scores.maintenance, None);
        assert_eq!(partial_metrics.scores.community, None);
        assert!(full_metrics.scores.maintenance.is_some());
        assert!(partial_metrics.repository.is_none());
    }

    #[test]
    fn archived_repository_scores_zero_maintenance() {
        let config = AuditConfig::default();
        let meta = crate_meta(900);
        let mut gh = github(900);
        gh.is_archived = true;
        let inputs = ScoringInputs {
            crate_meta: Some(&meta),
            github: Some(&gh),
            ..ScoringInputs::default()
        };
        let (score, metrics) = calculate_health_score(inputs, &config).unwrap();
        assert_eq!(metrics.scores.maintenance, Some(0.0));
        assert_eq!(determine_status(score, &config), HealthStatus::Risky);
    }

    #[test]
    fn yanked_versions_are_capped_at_ten() {
        let config = AuditConfig::default();
        let mut meta = crate_meta(5);
        meta.is_yanked = true;
        let gh = github(5);
        let inputs = ScoringInputs {
            crate_meta: Some(&meta),
            github: Some(&gh),
            ..ScoringInputs::default()
        };
        let (score, _) = calculate_health_score(inputs, &config).unwrap();
        assert!(score <= 10);
    }

    #[test]
    fn unknown_security_policy_is_not_penalised() {
        let meta = crate_meta(5);
        let mut unknown_policy = github(5);
        unknown_policy.has_security_policy = None;
        let mut no_policy = github(5);
        no_policy.has_security_policy = Some(false);

        let inputs = ScoringInputs {
            crate_meta: Some(&meta),
            github: Some(&unknown_policy),
            ..ScoringInputs::default()
        };
        assert_eq!(calculate_security_score(&inputs), None);

        let inputs = ScoringInputs {
            github: Some(&no_policy),
            ..inputs
        };
        assert_eq!(calculate_security_score(&inputs), Some(40.0));
    }

    #[test]
    fn future_timestamps_do_not_wrap() {
        assert_eq!(days_since(Utc::now() + Duration::days(3)), 0);
    }
}
