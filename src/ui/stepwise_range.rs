use eframe::egui::lerp;
use eframe::emath::inverse_lerp;
use itertools::Itertools;
use std::ops::RangeInclusive;

pub struct StepwiseRange<'a> {
    input_stops: &'a [f32],
    output_stops: &'a [f32],
}

impl<'a> StepwiseRange<'a> {
    pub fn new<'b, 'c>(input_stops: &'b [f32], output_stops: &'c [f32]) -> Self
    where
        'b: 'a,
        'c: 'a,
    {
        assert!(input_stops.len() >= 2);
        assert_eq!(input_stops.len(), output_stops.len());
        for (a, b) in input_stops.iter().tuple_windows() {
            assert!(a < b);
        }
        for (a, b) in output_stops.iter().tuple_windows() {
            assert!(a < b);
        }
        Self {
            input_stops,
            output_stops,
        }
    }

    pub fn input_range(&self) -> RangeInclusive<f32> {
        *self.input_stops.first().unwrap()..=*self.input_stops.last().unwrap()
    }

    pub fn output_range(&self) -> RangeInclusive<f32> {
        *self.output_stops.first().unwrap()..=*self.output_stops.last().unwrap()
    }

    pub fn lerp_out(&self, mut in_val: f32) -> f32 {
        let in_range = self.input_range();
        in_val = in_val.clamp(*in_range.start(), *in_range.end());
        for ((in_a, out_a), (in_b, out_b)) in self
            .input_stops
            .iter()
            .zip(self.output_stops.iter())
            .tuple_windows()
        {
            if *in_a <= in_val && in_val <= *in_b {
                return lerp(
                    *out_a..=*out_b,
                    inverse_lerp(*in_a..=*in_b, in_val).unwrap(),
                );
            }
        }
        f32::NAN
    }

    pub fn lerp_in(&self, out_val: f32) -> f32 {
        Self {
            input_stops: self.output_stops,
            output_stops: self.input_stops,
        }
        .lerp_out(out_val)
    }
}

#[cfg(test)]
mod test {
    use crate::ui::stepwise_range::StepwiseRange;

    #[test]
    fn test_range() {
        let range = StepwiseRange::new(&[0.0, 1.0, 2.0, 3.0], &[128.0, 256.0, 512.0, 1024.0]);

        assert_eq!(range.lerp_out(0.0), 128.0);
        assert_eq!(range.lerp_out(1.0), 256.0);
        assert_eq!(range.lerp_out(2.0), 512.0);
        assert_eq!(range.lerp_out(3.0), 1024.0);

        assert_eq!(range.lerp_in(128.0), 0.0);
        assert_eq!(range.lerp_in(256.0), 1.0);
        assert_eq!(range.lerp_in(512.0), 2.0);
        assert_eq!(range.lerp_in(1024.0), 3.0);

        assert_eq!(range.lerp_out(-1.0), 128.0);
        assert_eq!(range.lerp_out(4.0), 1024.0);

        assert_eq!(range.lerp_in(0.0), 0.0);
        assert_eq!(range.lerp_in(2048.0), 3.0);

        assert_eq!(range.lerp_out(0.5), 192.0);
        assert_eq!(range.lerp_out(1.5), 384.0);
        assert_eq!(range.lerp_out(2.5), 768.0);

        assert_eq!(range.lerp_in(192.0), 0.5);
        assert_eq!(range.lerp_in(384.0), 1.5);
        assert_eq!(range.lerp_in(768.0), 2.5);
    }
}
