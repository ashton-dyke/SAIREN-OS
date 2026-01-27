//! ML Engine for Optimal Drilling Conditions Analysis (V2.2)
//!
//! This module implements campaign-aware machine learning analysis to find
//! optimal drilling parameters (WOB, RPM, flow) for each formation type.
//!
//! ## Key Features
//! - Campaign-awareness (Production vs P&A optimization goals)
//! - **Dysfunction filtering** to reject unstable operating points (V2.2)
//! - **Stability-aware optimization** with grid-based binning (V2.2)
//! - Composite efficiency scoring (ROP + MSE + stability balance)
//! - Formation boundary detection and segmentation
//! - Statistical significance testing (p-value filtering via statrs)
//! - Multi-well/field-level knowledge transfer
//!
//! ## Architecture
//! - `quality_filter`: Data quality pre-filtering (WOB>5, RPM>40, etc.)
//! - `dysfunction_filter`: Reject samples with stick-slip, pack-off, founder (V2.2)
//! - `formation_segmenter`: Formation boundary detection (>15% d-exp shift)
//! - `correlations`: Pearson correlation with p-value testing (statrs)
//! - `optimal_finder`: Grid-based binning with stability penalty (V2.2)
//! - `analyzer`: Main orchestrator for ML analysis
//! - `scheduler`: Configurable interval scheduler (ML_INTERVAL_SECS)

pub mod quality_filter;
pub mod dysfunction_filter;
pub mod formation_segmenter;
pub mod correlations;
pub mod optimal_finder;
pub mod analyzer;
pub mod scheduler;
pub mod storage;

// Re-export public types
pub use quality_filter::DataQualityFilter;
pub use dysfunction_filter::DysfunctionFilter;
pub use formation_segmenter::FormationSegmenter;
pub use correlations::CorrelationEngine;
pub use optimal_finder::OptimalFinder;
pub use analyzer::HourlyAnalyzer;
pub use scheduler::{get_interval, get_interval_secs, MLScheduler};
pub use storage::{build_ml_context, MLInsightsStorage, MLStorageError};
