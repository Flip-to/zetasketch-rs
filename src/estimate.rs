//! Empirical HLL++ constants and bias correction, ported from ZetaSketch Java
//! `internal/hllplus/Data.java`.

use crate::bias_data::{BIAS_DATA, MEAN_DATA};

const MINIMUM_PRECISION: i32 = 4;
const MAXIMUM_PRECISION: i32 = 18;
const NUMBER_OF_NEIGHBORS_IN_KNN: usize = 6;

const LINEAR_COUNTING_THRESHOLD: [i32; 15] = [
    10, 20, 40, 80, 220, 400, 900, 1800, 3100, 6500, 11500, 20000, 50000, 120_000, 350_000,
];

/// `alpha_m` from the HLL paper.
pub(crate) fn alpha(precision: i32) -> f64 {
    match precision {
        4 => 0.673_101_517,
        5 => 0.697_121_585,
        6 => 0.709_209_798,
        _ => 0.7213 / (1.0 + 1.079 / f64::from(1i32 << precision)),
    }
}

/// Estimate at or below which linear counting is used instead of the HLL estimator.
pub(crate) fn linear_counting_threshold(precision: i32) -> i32 {
    if (MINIMUM_PRECISION..=MAXIMUM_PRECISION).contains(&precision) {
        return LINEAR_COUNTING_THRESHOLD[(precision - MINIMUM_PRECISION) as usize];
    }
    5 * (1 << precision) / 2
}

/// Bias of a raw estimate: k-nearest-neighbour (k = 6) inverse-distance-weighted mean.
pub(crate) fn estimate_bias(estimate: f64, precision: i32) -> f64 {
    if !(MINIMUM_PRECISION..=MAXIMUM_PRECISION).contains(&precision) {
        return 0.0;
    }
    let biases = BIAS_DATA[(precision - MINIMUM_PRECISION) as usize];
    let means = MEAN_DATA[(precision - MINIMUM_PRECISION) as usize];
    if estimate < means[0] || means[means.len() - 1] < estimate {
        return 0.0;
    }

    let (Ok(index) | Err(index)) = means.binary_search_by(|m| m.total_cmp(&estimate));
    let bottom = index.saturating_sub(NUMBER_OF_NEIGHBORS_IN_KNN);
    let top = if index > means.len() - NUMBER_OF_NEIGHBORS_IN_KNN {
        means.len()
    } else {
        index + NUMBER_OF_NEIGHBORS_IN_KNN
    };

    let mut weighted: Vec<(f64, f64)> = (bottom..top)
        .map(|i| {
            let d = means[i] - estimate;
            (biases[i], d * d)
        })
        .collect();
    // Stable, like Java's Collections.sort, so equal distances keep table order.
    weighted.sort_by(|a, b| a.1.total_cmp(&b.1));
    weighted.truncate(NUMBER_OF_NEIGHBORS_IN_KNN);

    if weighted[0].1 == 0.0 {
        return weighted[0].0;
    }
    let mut sum = 0.0;
    let mut total_weight = 0.0;
    for (bias, distance) in weighted {
        total_weight += 1.0 / distance;
        sum += bias / distance;
    }
    sum / total_weight
}

/// Java `Math.round(double)`: round half up, toward positive infinity.
pub(crate) fn java_round(x: f64) -> i64 {
    let f = x.floor();
    if x - f >= 0.5 {
        f as i64 + 1
    } else {
        f as i64
    }
}
