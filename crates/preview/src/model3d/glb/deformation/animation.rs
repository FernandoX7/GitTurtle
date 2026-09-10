//! glTF sampler evaluation. Quaternion LINEAR uses shortest-path slerp;
//! CUBICSPLINE uses component Hermite interpolation and then normalizes.
use super::*;

const MAX_CLIPS: usize = 64;
const MAX_CHANNELS: usize = 256;
const MAX_KEYFRAMES: usize = 100_000;
const MAX_ANIMATION_COMPONENTS: usize = 1_000_000;
const MAX_DURATION_SECONDS: f64 = 86_400.;

#[derive(Deserialize)]
struct Animation {
    name: Option<String>,
    channels: Vec<RawChannel>,
    samplers: Vec<RawSampler>,
}
#[derive(Deserialize)]
struct RawChannel {
    sampler: usize,
    target: Target,
}
#[derive(Deserialize)]
struct Target {
    node: Option<usize>,
    path: String,
}
#[derive(Deserialize)]
struct RawSampler {
    input: usize,
    output: usize,
    interpolation: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Property {
    Translation,
    Rotation,
    Scale,
    Weights,
}
#[derive(Clone, Copy, Debug)]
enum Interpolation {
    Linear,
    Step,
    Cubic,
}

#[derive(Debug)]
pub(super) struct Channel {
    pub(super) node: usize,
    property: Property,
    interpolation: Interpolation,
    components: usize,
    times: Vec<f64>,
    values: Vec<f64>,
}
#[derive(Debug)]
pub(super) struct Clip {
    pub(super) metadata: ModelAnimationClip,
    pub(super) channels: Vec<Channel>,
}
impl Clip {
    pub(super) fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.metadata.name.capacity()
            + self.channels.capacity() * std::mem::size_of::<Channel>()
            + self
                .channels
                .iter()
                .map(|c| (c.times.capacity() + c.values.capacity()) * 8)
                .sum::<usize>()
    }
}

impl<F: Fn() -> Result<()>> Decoder<'_, F> {
    pub(super) fn animation_clips(&mut self, details: &mut Vec<String>) -> Result<Vec<Clip>> {
        let mut clips = Vec::new();
        let mut keyframes = 0usize;
        let mut components = 0usize;
        if self.document.animations.len() > MAX_CLIPS {
            details.push("Animation playback unavailable: asset exceeds the 64-clip preview limit; authored default geometry remains available.".into());
            return Ok(clips);
        }
        for (index, raw) in self.document.animations.iter().enumerate() {
            (self.check)()?;
            let result = self.animation_clip(raw, index, &mut keyframes, &mut components);
            match result {
                Ok(clip) => clips.push(clip),
                Err(error) => {
                    // Propagate cancellation even when a malformed optional clip
                    // is otherwise allowed to fall back to correct default pose.
                    (self.check)()?;
                    let reason: String = format!("{error:#}").chars().take(512).collect();
                    details.push(format!("Animation clip {} unavailable: {reason}. Authored default geometry remains available.", index + 1));
                }
            }
        }
        Ok(clips)
    }
    fn animation_clip(
        &mut self,
        raw: &serde_json::Value,
        index: usize,
        keyframes: &mut usize,
        components: &mut usize,
    ) -> Result<Clip> {
        let source: Animation =
            serde_json::from_value(raw.clone()).context("malformed animation metadata")?;
        ensure!(
            !source.channels.is_empty()
                && source.channels.len() <= MAX_CHANNELS
                && !source.samplers.is_empty()
                && source.samplers.len() <= MAX_CHANNELS,
            "animation exceeds the 256-channel/sampler limit or contains no channels/samplers"
        );
        let name: String = source
            .name
            .as_deref()
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_control())
            .scan(0usize, |bytes, ch| {
                *bytes += ch.len_utf8();
                (*bytes <= 256).then_some(ch)
            })
            .collect();
        let name = if name.is_empty() {
            format!("Clip {}", index + 1)
        } else {
            name
        };
        let mut channels = Vec::new();
        let mut targets = HashSet::new();
        let mut duration = 0f64;
        for raw in source.channels {
            (self.check)()?;
            // glTF defines absent target.node as an ignored channel.
            let Some(node_index) = raw.target.node else {
                continue;
            };
            let node = self
                .document
                .nodes
                .get(node_index)
                .context("animation target node is missing")?;
            ensure!(
                node.matrix.is_none(),
                "animation cannot target a node with a matrix transform"
            );
            let (property, count, kind) = match raw.target.path.as_str() {
                "translation" => (Property::Translation, 3, "VEC3"),
                "rotation" => (Property::Rotation, 4, "VEC4"),
                "scale" => (Property::Scale, 3, "VEC3"),
                "weights" => {
                    let mesh = node
                        .mesh
                        .and_then(|v| self.document.meshes.get(v))
                        .context("animation weights require a mesh with morph targets")?;
                    let count = mesh.primitives.first().map_or(0, |v| v.targets.len());
                    ensure!(
                        count > 0 && count <= MAX_MORPH_TARGETS,
                        "animation weights require supported morph targets"
                    );
                    (Property::Weights, count, "SCALAR")
                }
                _ => bail!("unsupported animation target path {}", raw.target.path),
            };
            ensure!(
                targets.insert((node_index, property)),
                "duplicate animation channel target"
            );
            let sampler = source
                .samplers
                .get(raw.sampler)
                .context("animation references a missing sampler")?;
            let interpolation = match sampler.interpolation.as_deref().unwrap_or("LINEAR") {
                "LINEAR" => Interpolation::Linear,
                "STEP" => Interpolation::Step,
                "CUBICSPLINE" => Interpolation::Cubic,
                _ => bail!("unsupported animation interpolation"),
            };
            let input = self
                .document
                .accessors
                .get(sampler.input)
                .context("animation input accessor is missing")?;
            ensure!(
                input.kind == "SCALAR" && input.component_type == 5126 && !input.normalized,
                "animation timestamps require non-normalized FLOAT SCALAR"
            );
            ensure!(
                input.min.is_some() && input.max.is_some(),
                "animation timestamps require declared min/max bounds"
            );
            let input_count = input.count;
            *keyframes = keyframes
                .checked_add(input_count)
                .context("animation keyframe work overflow")?;
            ensure!(
                *keyframes <= MAX_KEYFRAMES,
                "animation exceeds the 100,000 cumulative channel-keyframe limit"
            );
            if matches!(interpolation, Interpolation::Cubic) {
                ensure!(input_count >= 2, "CUBICSPLINE requires at least two keys");
            }
            let output = self
                .document
                .accessors
                .get(sampler.output)
                .context("animation output accessor is missing")?;
            let normalized_integer = matches!(property, Property::Rotation | Property::Weights)
                && matches!(output.component_type, 5120..=5123)
                && output.normalized;
            ensure!(
                output.kind == kind
                    && ((output.component_type == 5126 && !output.normalized)
                        || normalized_integer),
                "animation output accessor has an unsupported shape or component type"
            );
            let factor = if matches!(interpolation, Interpolation::Cubic) {
                3
            } else {
                1
            };
            let expected = input_count
                * factor
                * if property == Property::Weights {
                    count
                } else {
                    1
                };
            ensure!(
                output.count == expected,
                "animation output count does not match channel keyframes"
            );
            *components = components
                .checked_add(input_count * count * factor + input_count)
                .context("animation component work overflow")?;
            ensure!(
                *components <= MAX_ANIMATION_COMPONENTS,
                "animation exceeds the 1,000,000 retained component limit"
            );
            for accessor in [input, output] {
                if let Some(view_index) = accessor.buffer_view {
                    let source = view(self.document, view_index)?;
                    ensure!(
                        source.target.is_none() && source.byte_stride.is_none(),
                        "animation accessors must use tightly packed non-vertex buffer views"
                    );
                }
            }
            let times = self.deformation_values(sampler.input, 1)?;
            ensure!(
                times.iter().all(|v| *v >= 0. && *v <= MAX_DURATION_SECONDS)
                    && times.windows(2).all(|v| v[0] < v[1]),
                "animation timestamps must be strictly increasing within 0..86,400 seconds"
            );
            let values = self.deformation_values(
                sampler.output,
                if property == Property::Weights {
                    1
                } else {
                    count
                },
            )?;
            let channel = Channel {
                node: node_index,
                property,
                interpolation,
                components: count,
                times,
                values,
            };
            for key in 0..input_count {
                if key.is_multiple_of(256) {
                    (self.check)()?;
                }
                let value = channel.key(key, 1);
                if property == Property::Rotation {
                    let length: f64 = value.iter().map(|v| v * v).sum();
                    let tolerance = if normalized_integer { 0.02 } else { 1e-4 };
                    ensure!(
                        (length - 1.).abs() <= tolerance,
                        "animation rotation values must be unit quaternions"
                    );
                } else if property == Property::Weights {
                    validate_weights(value, count)?;
                }
            }
            duration = duration.max(*channel.times.last().unwrap());
            channels.push(channel);
        }
        ensure!(
            !channels.is_empty(),
            "animation has no supported node channels"
        );
        Ok(Clip {
            metadata: ModelAnimationClip {
                name,
                duration_seconds: duration,
            },
            channels,
        })
    }
}

impl Channel {
    /// Cubic layouts store in-tangent, value, out-tangent per key.
    fn key(&self, index: usize, slot: usize) -> &[f64] {
        let offset = if matches!(self.interpolation, Interpolation::Cubic) {
            (index * 3 + slot) * self.components
        } else {
            index * self.components
        };
        &self.values[offset..offset + self.components]
    }
    fn sample(&self, time: f64) -> Result<Vec<f64>> {
        let right = self.times.partition_point(|&v| v <= time);
        let mut value = if right == 0 {
            self.key(0, 1).to_vec()
        } else if right >= self.times.len() {
            self.key(self.times.len() - 1, 1).to_vec()
        } else {
            let left = right - 1;
            let a = self.key(left, 1);
            let b = self.key(right, 1);
            let dt = self.times[right] - self.times[left];
            let t = ((time - self.times[left]) / dt).clamp(0., 1.);
            match self.interpolation {
                Interpolation::Step => a.to_vec(),
                Interpolation::Linear if self.property == Property::Rotation => slerp(a, b, t)?,
                Interpolation::Linear => {
                    a.iter().zip(b).map(|(a, b)| a * (1. - t) + b * t).collect()
                }
                Interpolation::Cubic => {
                    let out = self.key(left, 2);
                    let input = self.key(right, 0);
                    let t2 = t * t;
                    let t3 = t2 * t;
                    (0..self.components)
                        .map(|i| {
                            (2. * t3 - 3. * t2 + 1.) * a[i]
                                + (t3 - 2. * t2 + t) * dt * out[i]
                                + (-2. * t3 + 3. * t2) * b[i]
                                + (t3 - t2) * dt * input[i]
                        })
                        .collect()
                }
            }
        };
        ensure!(
            value.iter().all(|v| v.is_finite() && v.abs() <= 1e12),
            "GLB animation interpolation produced unreasonable values"
        );
        if self.property == Property::Rotation {
            normalize(&mut value)?;
        }
        Ok(value)
    }
    pub(super) fn apply(&self, node: &mut Node, time: f64) -> Result<()> {
        let value = self.sample(time)?;
        match self.property {
            Property::Translation => node.translation = Some(value.as_slice().try_into().unwrap()),
            Property::Rotation => node.rotation = Some(value.as_slice().try_into().unwrap()),
            Property::Scale => node.scale = Some(value.as_slice().try_into().unwrap()),
            Property::Weights => node.weights = Some(value),
        }
        Ok(())
    }
}
fn normalize(q: &mut [f64]) -> Result<()> {
    let length = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    ensure!(
        length.is_finite() && length > 1e-12,
        "GLB interpolated quaternion has zero or invalid length"
    );
    for v in q {
        *v /= length;
    }
    Ok(())
}
fn slerp(a: &[f64], b: &[f64], t: f64) -> Result<Vec<f64>> {
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    normalize(&mut a)?;
    normalize(&mut b)?;
    let mut cosine: f64 = a.iter().zip(&b).map(|(a, b)| a * b).sum();
    if cosine < 0. {
        cosine = -cosine;
        for v in &mut b {
            *v = -*v;
        }
    }
    if cosine > 0.9995 {
        return Ok(a
            .iter()
            .zip(&b)
            .map(|(a, b)| a * (1. - t) + b * t)
            .collect());
    }
    let angle = cosine.clamp(-1., 1.).acos();
    let denominator = angle.sin();
    let first = ((1. - t) * angle).sin() / denominator;
    let second = (t * angle).sin() / denominator;
    Ok(a.iter()
        .zip(&b)
        .map(|(a, b)| a * first + b * second)
        .collect())
}
