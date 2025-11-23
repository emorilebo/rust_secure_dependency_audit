//! Health scoring algorithms for dependencies

use crate::config::AuditConfig;
use crate::metadata::{CrateMetadata, GitHubMetadata, GitLabMetadata};
use crate::types::{ComponentScores, DependencyMetrics, HealthStatus, RepositoryMetrics};
use chrono::Utc;

/// Calculate overall health score for a dependency
pub fn calculate_health_score(
    crate_meta: Option<&CrateMetadata>,
    github_meta: Option<&GitHubMetadata>,
    gitlab_meta: Option<&GitLabMetadata>,
    config: &AuditConfig,
) -> (u8, ComponentScores, Option<DependencyMetrics>) {
    let weights = &config.scoring_weights;
    
    // Calculate component scores
    let recency_score = calculate_recency_score(crate_meta, github_meta, gitlab_meta, config);
    let maintenance_score = calculate_maintenance_score(github_meta, gitlab_meta);
    let community_score = calculate_community_score(crate_meta, github_meta, gitlab_meta);
    let stability_score = calculate_stability_score(crate_meta);
    
    let scores = ComponentScores {
        recency: recency_score,
        maintenance: maintenance_score,
        community: community_score,
        stability: stability_score,
    };
    
    // Calculate weighted overall score
    let overall = (recency_score * weights.recency
        + maintenance_score * weights.maintenance
        + community_score * weights.community
        + stability_score * weights.stability)
        .round()
        .clamp(0.0, 100.0) as u8;
    
    // Build metrics
    let metrics = build_metrics(crate_meta, github_meta, gitlab_meta, &scores);
    
    (overall, scores, metrics)
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

/// Calculate recency score based on last update
fn calculate_recency_score(
    crate_meta: Option<&CrateMetadata>,
    github_meta: Option<&GitHubMetadata>,
    gitlab_meta: Option<&GitLabMetadata>,
    config: &AuditConfig,
) -> f32 {
    let now = Utc::now();
    
    // Prefer git repository last push over crates.io publish date
    let last_update = if let Some(gh) = github_meta {
        gh.pushed_at
    } else if let Some(gl) = gitlab_meta {
        gl.last_activity_at
    } else if let Some(cr) = crate_meta {
        cr.updated_at
    } else {
        return 0.0; // No data
    };
    
    let days_old = (now - last_update).num_days() as u32;
    
    // Score based on staleness thresholds
    let stale_days = config.staleness_thresholds.stale_days;
    let risky_days = config.staleness_thresholds.risky_days;
    
    if days_old <= 30 {
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
    }
}

/// Calculate maintenance score from repository activity
fn calculate_maintenance_score(
    github_meta: Option<&GitHubMetadata>,
    gitlab_meta: Option<&GitLabMetadata>,
) -> f32 {
    // Base score if we have repository data
    let mut score: f32 = 50.0;
    
    if let Some(gh) = github_meta {
        // Archived repo is a major red flag
        if gh.is_archived {
            return 0.0;
        }
        
        // Low open issues is good
        if gh.open_issues < 10 {
            score += 25.0;
        } else if gh.open_issues < 50 {
            score += 10.0;
        } else if gh.open_issues > 200 {
            score -= 10.0;
        }
        
        // Recent activity is good
        let days_since_push = (Utc::now() - gh.pushed_at).num_days();
        if days_since_push <= 30 {
            score += 25.0;
        } else if days_since_push <= 90 {
            score += 15.0;
        } else if days_since_push > 365 {
            score -= 20.0;
        }
    } else if let Some(gl) = gitlab_meta {
        if gl.is_archived {
            return 0.0;
        }
        
        if gl.open_issues < 10 {
            score += 25.0;
        } else if gl.open_issues < 50 {
            score += 10.0;
        }
        
        let days_since_activity = (Utc::now() - gl.last_activity_at).num_days();
        if days_since_activity <= 30 {
            score += 25.0;
        } else if days_since_activity <= 90 {
            score += 15.0;
        } else if days_since_activity > 365 {
            score -= 20.0;
        }
    } else {
        // No repo data, moderate score
        return 50.0;
    }
    
    score.clamp(0.0, 100.0)
}

/// Calculate community score from contributors/maintainers
fn calculate_community_score(
    crate_meta: Option<&CrateMetadata>,
    github_meta: Option<&GitHubMetadata>,
    gitlab_meta: Option<&GitLabMetadata>,
) -> f32 {
    let mut score: f32 = 0.0;
    
    // Author/maintainer count from crates.io
    if let Some(crate_meta) = crate_meta {
        let author_count = crate_meta.authors.len() as u32;
        score += match author_count {
            0 => 0.0,
            1 => 30.0,
            2..=5 => 50.0,
            6..=10 => 70.0,
            _ => 80.0,
        };
    }
    
    // GitHub metrics
    if let Some(gh) = github_meta {
        // Stars indicate popularity
        score += match gh.stars {
            0..=10 => 0.0,
            11..=50 => 10.0,
            51..=200 => 20.0,
            201..=1000 => 30.0,
            _ => 40.0,
        };
        
        // Contributors
        if let Some(contributors) = gh.contributors_count {
            score += match contributors {
                0..=1 => 0.0,
                2..=5 => 10.0,
                6..=20 => 20.0,
                _ => 30.0,
            };
        }
    } else if let Some(gl) = gitlab_meta {
        score += match gl.stars {
            0..=10 => 0.0,
            11..=50 => 10.0,
            51..=200 => 20.0,
            201..=1000 => 30.0,
            _ => 40.0,
        };
    }
    
    score.clamp(0.0, 100.0)
}

/// Calculate stability score from version history
fn calculate_stability_score(crate_meta: Option<&CrateMetadata>) -> f32 {
    if let Some(meta) = crate_meta {
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
        
        (score + download_bonus).clamp(0.0, 100.0)
    } else {
        50.0 // Unknown
    }
}

/// Build detailed metrics object
fn build_metrics(
    crate_meta: Option<&CrateMetadata>,
    github_meta: Option<&GitHubMetadata>,
    gitlab_meta: Option<&GitLabMetadata>,
    scores: &ComponentScores,
) -> Option<DependencyMetrics> {
    let now = Utc::now();
    
    let days_since_last_update = if let Some(gh) = github_meta {
        Some((now - gh.pushed_at).num_days() as u32)
    } else if let Some(gl) = gitlab_meta {
        Some((now - gl.last_activity_at).num_days() as u32)
    } else if let Some(cr) = crate_meta {
        Some((now - cr.updated_at).num_days() as u32)
    } else {
        None
    };
    
    let repository = if let Some(gh) = github_meta {
        Some(RepositoryMetrics {
            open_issues: Some(gh.open_issues),
            contributor_count: gh.contributors_count,
            days_since_last_commit: Some((now - gh.pushed_at).num_days() as u32),
            stars: Some(gh.stars),
            is_archived: Some(gh.is_archived),
        })
    } else if let Some(gl) = gitlab_meta {
        Some(RepositoryMetrics {
            open_issues: Some(gl.open_issues),
            contributor_count: None,
            days_since_last_commit: Some((now - gl.last_activity_at).num_days() as u32),
            stars: Some(gl.stars),
            is_archived: Some(gl.is_archived),
        })
    } else {
        None
    };
    
    Some(DependencyMetrics {
        days_since_last_update,
        version_count: crate_meta.map(|m| m.version_count),
        maintainer_count: crate_meta.map(|m| m.authors.len() as u32),
        repository,
        scores: scores.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_recency_score_recent() {
        let mut config = AuditConfig::default();
        let crate_meta = CrateMetadata {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            license: None,
            repository: None,
            homepage: None,
            downloads: 1000,
            recent_downloads: None,
            created_at: Utc::now() - Duration::days(365),
            updated_at: Utc::now() - Duration::days(15),
            version_count: 10,
            authors: vec![],
        };
        
        let score = calculate_recency_score(Some(&crate_meta), None, None, &config);
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
}
