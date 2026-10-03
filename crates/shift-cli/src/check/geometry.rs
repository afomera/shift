//! Outline geometry measurements used by health checks.
//!
//! Coordinates are font units with y increasing upward. Contour direction
//! follows the PostScript/UFO convention that Shift sources use and that the
//! TrueType compiler reverses on output: outer contours run counter-clockwise
//! (positive signed area) and counters run clockwise.

use shift_font::curve::segment_bounds;
use shift_font::{CurveSegmentIter, Point};

/// Axis-aligned extent of a set of points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Extent {
    pub fn of<'a>(points: impl IntoIterator<Item = &'a Point>) -> Option<Self> {
        let mut points = points.into_iter();
        let first = points.next()?;
        let mut extent = Self {
            min_x: first.x(),
            min_y: first.y(),
            max_x: first.x(),
            max_y: first.y(),
        };
        for point in points {
            extent.min_x = extent.min_x.min(point.x());
            extent.min_y = extent.min_y.min(point.y());
            extent.max_x = extent.max_x.max(point.x());
            extent.max_y = extent.max_y.max(point.y());
        }
        Some(extent)
    }

    /// Returns the tight extent of drawn outlines, including curve extrema.
    ///
    /// On-curve points are always included, so contours whose point cycle
    /// starts off-curve still contribute their anchors to the extent.
    pub fn of_outlines<'a>(
        contours: impl IntoIterator<Item = (&'a [Point], bool)>,
    ) -> Option<Self> {
        let mut extent: Option<Self> = None;
        for (points, closed) in contours {
            let on_curve = Self::of(points.iter().filter(|point| point.is_on_curve()));
            let segments = CurveSegmentIter::new(points, closed).map(|segment| {
                let (min_x, min_y, max_x, max_y) = segment_bounds(&segment);
                Self {
                    min_x,
                    min_y,
                    max_x,
                    max_y,
                }
            });
            for part in on_curve.into_iter().chain(segments) {
                extent = Some(match extent {
                    Some(extent) => extent.union(&part),
                    None => part,
                });
            }
        }
        extent
    }

    fn union(&self, other: &Self) -> Self {
        Self {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }

    fn contains(&self, other: &Self) -> bool {
        self.min_x <= other.min_x
            && self.min_y <= other.min_y
            && other.max_x <= self.max_x
            && other.max_y <= self.max_y
    }
}

/// Indices of closed contours whose winding contradicts their nesting depth.
///
/// Signed area is measured on the control polygon (on- and off-curve points),
/// which preserves the winding of well-formed curves. A contour counts as
/// nested inside another only when its extent lies within the other's extent
/// and its first point lies inside the other's polygon; this keeps overlapping
/// (unmerged) outer strokes from being treated as counters. Contours with
/// fewer than three points or zero area are ignored. Even depth expects
/// counter-clockwise winding and odd depth expects clockwise winding.
pub fn reversed_contours(contours: &[&[Point]]) -> Vec<usize> {
    let measured = contours
        .iter()
        .enumerate()
        .filter(|(_, points)| points.len() >= 3)
        .filter_map(|(index, points)| {
            let area = signed_area(points);
            let extent = Extent::of(points.iter())?;
            (area.abs() > f64::EPSILON).then_some((index, *points, area, extent))
        })
        .collect::<Vec<_>>();

    measured
        .iter()
        .filter(|(index, points, area, extent)| {
            let probe = &points[0];
            let depth = measured
                .iter()
                .filter(|(other_index, other_points, _, other_extent)| {
                    other_index != index
                        && other_extent.contains(extent)
                        && contains_point(other_points, probe.x(), probe.y())
                })
                .count();
            let expects_counter_clockwise = depth % 2 == 0;
            (*area > 0.0) != expects_counter_clockwise
        })
        .map(|(index, ..)| *index)
        .collect()
}

fn signed_area(points: &[Point]) -> f64 {
    let mut twice_area = 0.0;
    for (index, point) in points.iter().enumerate() {
        let next = &points[(index + 1) % points.len()];
        twice_area += point.x() * next.y() - next.x() * point.y();
    }
    twice_area / 2.0
}

fn contains_point(polygon: &[Point], x: f64, y: f64) -> bool {
    let mut inside = false;
    let mut previous = &polygon[polygon.len() - 1];
    for current in polygon {
        let crosses = (current.y() > y) != (previous.y() > y);
        if crosses {
            let intersection_x = (previous.x() - current.x()) * (y - current.y())
                / (previous.y() - current.y())
                + current.x();
            if x < intersection_x {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle(min: f64, max: f64, counter_clockwise: bool) -> Vec<Point> {
        let mut points = vec![
            Point::on_curve(min, min),
            Point::on_curve(max, min),
            Point::on_curve(max, max),
            Point::on_curve(min, max),
        ];
        if !counter_clockwise {
            points.reverse();
        }
        points
    }

    #[test]
    fn outline_extent_uses_curve_extrema_not_control_points() {
        let points = vec![
            Point::on_curve(0.0, 0.0),
            Point::off_curve(0.0, 100.0),
            Point::off_curve(100.0, 100.0),
            Point::on_curve(100.0, 0.0),
        ];

        let extent = Extent::of_outlines([(points.as_slice(), true)]).unwrap();

        assert_eq!(extent.max_y, 75.0);
        assert_eq!(extent.max_x, 100.0);
    }

    #[test]
    fn postscript_outer_and_counter_are_consistent() {
        let outer = rectangle(0.0, 100.0, true);
        let counter = rectangle(25.0, 75.0, false);

        assert!(reversed_contours(&[&outer, &counter]).is_empty());
    }

    #[test]
    fn counter_with_outer_winding_is_reported() {
        let outer = rectangle(0.0, 100.0, true);
        let counter = rectangle(25.0, 75.0, true);

        assert_eq!(reversed_contours(&[&outer, &counter]), vec![1]);
    }

    #[test]
    fn truetype_direction_reports_every_contour() {
        let outer = rectangle(0.0, 100.0, false);
        let counter = rectangle(25.0, 75.0, true);

        assert_eq!(reversed_contours(&[&outer, &counter]), vec![0, 1]);
    }

    #[test]
    fn overlapping_outer_strokes_are_not_counters() {
        let left = rectangle(0.0, 100.0, true);
        let right = rectangle(50.0, 150.0, true);

        assert!(reversed_contours(&[&left, &right]).is_empty());
    }
}
