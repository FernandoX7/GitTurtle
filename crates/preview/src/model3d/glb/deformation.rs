//! Immutable, bounded glTF pose data. Morph positions precede skinning; skins
//! place vertices with world(joint) * inverseBind, independently of mesh TRS.
use super::super::appearance::TriangleAppearance;
use super::*;

mod animation;
use animation::Clip;

const MAX_JOINTS_PER_SKIN: usize = 256;
const MAX_JOINTS: usize = 1_024;
const MAX_MORPH_TARGETS: usize = 8;
const MAX_DEFORMATION_COMPONENTS: usize = 4_000_000;
const MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;
const MAX_FRAME_WORK: usize = 8_000_000;

#[derive(Clone, Debug)]
pub struct ModelAnimationClip {
    pub name: String,
    pub duration_seconds: f64,
}

#[derive(Debug)]
pub(in crate::model3d) struct GlbFrame {
    pub triangles: Vec<Triangle>,
    pub reverse_winding: Vec<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Skin {
    joints: Vec<usize>,
    pub(super) inverse_bind_matrices: Option<usize>,
    skeleton: Option<usize>,
}

#[derive(Debug)]
struct RetainedSkin {
    joints: Vec<usize>,
    inverse_bind: Vec<Matrix>,
}

#[derive(Debug)]
struct Influence {
    joints: [u16; 8],
    weights: [f64; 8],
}

#[derive(Debug)]
struct RetainedPrimitive {
    positions: Vec<Point>,
    indices: Vec<usize>,
    morphs: Vec<Vec<Point>>,
    influences: Option<Vec<Influence>>,
}

#[derive(Debug)]
pub(super) struct RetainedMesh {
    primitives: Vec<RetainedPrimitive>,
    weights: Vec<f64>,
    appearance: Vec<TriangleAppearance>,
}

/// Source data contain no document, captured bytes, decoded views or file handles.
/// Frames share this immutable data; only requested poses allocate output geometry.
#[derive(Debug)]
pub(in crate::model3d) struct GlbAnimation {
    nodes: Vec<Node>,
    transforms: Vec<Matrix>,
    /// Parent-before-child for all nodes, including off-scene joint ancestors.
    hierarchy: Vec<(usize, Option<usize>)>,
    selected: Vec<usize>,
    meshes: HashMap<usize, RetainedMesh>,
    skins: HashMap<usize, RetainedSkin>,
    clips: Vec<Clip>,
    metadata: Vec<ModelAnimationClip>,
    retained_bytes: usize,
    triangle_count: usize,
}

pub(super) fn validate_mesh_weights(mesh: &Mesh) -> Result<()> {
    let count = mesh.primitives.first().map_or(0, |v| v.targets.len());
    ensure!(
        count <= MAX_MORPH_TARGETS,
        "GLB exceeds the 8 morph-target preview limit"
    );
    ensure!(
        mesh.primitives.iter().all(|v| v.targets.len() == count),
        "GLB mesh primitives have inconsistent morph target counts"
    );
    if let Some(weights) = &mesh.weights {
        validate_weights(weights, count)?;
    }
    Ok(())
}

fn validate_weights(weights: &[f64], count: usize) -> Result<()> {
    ensure!(
        weights.len() == count && weights.iter().all(|v| v.is_finite() && v.abs() <= 1e6),
        "GLB morph weights have invalid count or unreasonable values"
    );
    Ok(())
}

pub(super) fn validate_node(document: &Document, node: &Node) -> Result<()> {
    if let Some(skin) = node.skin {
        ensure!(
            skin < document.skins.len() && node.mesh.is_some(),
            "GLB skin references a missing skin or has no mesh"
        );
    }
    if let Some(weights) = &node.weights {
        let mesh = document
            .meshes
            .get(node.mesh.context("GLB morph weights require a mesh")?)
            .context("GLB node references a missing mesh")?;
        validate_weights(
            weights,
            mesh.primitives.first().map_or(0, |v| v.targets.len()),
        )?;
    }
    Ok(())
}

impl<F: Fn() -> Result<()>> Decoder<'_, F> {
    fn retain(&mut self, bytes: usize) -> Result<()> {
        self.retained_deformation_bytes = self
            .retained_deformation_bytes
            .checked_add(bytes)
            .context("GLB retained deformation size overflow")?;
        ensure!(
            self.retained_deformation_bytes <= MAX_RETAINED_BYTES,
            "GLB exceeds the 64 MiB retained deformation preview limit"
        );
        Ok(())
    }

    /// Generic scalar/vector/MAT4 decoding used only after property-specific
    /// shape/type checks. Sparse overrides and meshopt views share one reader.
    fn deformation_values(&mut self, index: usize, components: usize) -> Result<Vec<f64>> {
        self.prepare_accessor(index)?;
        let accessor = self
            .document
            .accessors
            .get(index)
            .context("GLB deformation references a missing accessor")?;
        let count = accessor
            .count
            .checked_mul(components)
            .context("GLB deformation component overflow")?;
        self.deformation_components = self
            .deformation_components
            .checked_add(count)
            .context("GLB deformation work overflow")?;
        ensure!(
            self.deformation_components <= MAX_DEFORMATION_COMPONENTS,
            "GLB exceeds the 4,000,000 deformation-component decode limit"
        );
        self.retain(count * std::mem::size_of::<f64>())?;
        let accessor = &self.document.accessors[index];
        let size = component_size(accessor.component_type)?;
        let element = size * components;
        let mut output = vec![0.; count];
        let read = |data: &[u8], offset: usize, target: &mut [f64]| -> Result<()> {
            for (i, value) in target.iter_mut().enumerate() {
                let start = offset + i * size;
                let bytes = data
                    .get(start..start + size)
                    .context("Truncated GLB deformation accessor")?;
                let (raw, divisor) = match accessor.component_type {
                    5120 => (bytes[0] as i8 as f64, 127.),
                    5121 => (bytes[0] as f64, 255.),
                    5122 => (i16::from_le_bytes(bytes.try_into().unwrap()) as f64, 32767.),
                    5123 => (u16::from_le_bytes(bytes.try_into().unwrap()) as f64, 65535.),
                    5126 => (f32::from_le_bytes(bytes.try_into().unwrap()) as f64, 1.),
                    _ => bail!("Unsupported GLB deformation component type"),
                };
                *value = if accessor.normalized {
                    (raw / divisor).max(-1.)
                } else {
                    raw
                };
                ensure!(
                    value.is_finite() && value.abs() <= 1e12,
                    "GLB deformation contains nonfinite or unreasonable values"
                );
            }
            Ok(())
        };
        if let Some(index) = accessor.buffer_view {
            let source = view(self.document, index)?;
            let stride = source.byte_stride.unwrap_or(element);
            ensure!(
                stride >= element,
                "GLB deformation accessor stride is too small"
            );
            let data = self.data(index)?;
            for (i, target) in output.chunks_exact_mut(components).enumerate() {
                if i.is_multiple_of(256) {
                    (self.check)()?;
                }
                read(data, accessor.byte_offset + i * stride, target)?;
            }
        }
        if let Some(sparse) = &accessor.sparse {
            let indices = self.sparse_indices(accessor)?;
            let data = self.data(sparse.values.buffer_view)?;
            for (i, index) in indices.into_iter().enumerate() {
                if i.is_multiple_of(256) {
                    (self.check)()?;
                }
                read(
                    data,
                    sparse.values.byte_offset + i * element,
                    &mut output[index * components..(index + 1) * components],
                )?;
            }
        }
        Ok(output)
    }

    pub(super) fn deformation_source(
        &mut self,
        roots: &[usize],
        transforms: Vec<Matrix>,
        details: &mut Vec<String>,
    ) -> Result<GlbAnimation> {
        fn visit(
            nodes: &[Node],
            node: usize,
            parent: Option<usize>,
            out: &mut Vec<(usize, Option<usize>)>,
        ) {
            out.push((node, parent));
            for &child in &nodes[node].children {
                visit(nodes, child, Some(node), out);
            }
        }
        let mut selected_order = Vec::new();
        for &root in roots {
            visit(&self.document.nodes, root, None, &mut selected_order);
        }
        ensure!(
            selected_order.len() <= MAX_OBJECTS,
            "GLB exceeds node instance preview limit"
        );
        let selected: Vec<usize> = selected_order.into_iter().map(|v| v.0).collect();
        let mut parents = vec![false; self.document.nodes.len()];
        for node in &self.document.nodes {
            for &child in &node.children {
                parents[child] = true;
            }
        }
        let mut hierarchy = Vec::new();
        for (i, &has_parent) in parents.iter().enumerate() {
            if !has_parent {
                visit(&self.document.nodes, i, None, &mut hierarchy);
            }
        }
        let mut root_of = vec![0usize; self.document.nodes.len()];
        let mut parent_of = vec![None; self.document.nodes.len()];
        for &(node, parent) in &hierarchy {
            root_of[node] = parent.map_or(node, |parent| root_of[parent]);
            parent_of[node] = parent;
        }
        let selected_set: HashSet<usize> = selected.iter().copied().collect();
        let mut skins = HashMap::new();
        let mut joint_count = 0;
        let mut triangle_count = 0usize;
        let mut frame_work = 0usize;
        for &index in &selected {
            (self.check)()?;
            let node = &self.document.nodes[index];
            let Some(mesh_index) = node.mesh else {
                continue;
            };
            if !self.meshes.contains_key(&mesh_index) {
                let mesh = self.decode_mesh(mesh_index)?;
                self.meshes.insert(mesh_index, mesh);
            }
            if let Some(skin_index) = node.skin
                && let std::collections::hash_map::Entry::Vacant(entry) = skins.entry(skin_index)
            {
                let skin = &self.document.skins[skin_index];
                joint_count += skin.joints.len();
                ensure!(
                    !skin.joints.is_empty()
                        && skin.joints.len() <= MAX_JOINTS_PER_SKIN
                        && joint_count <= MAX_JOINTS,
                    "GLB exceeds the 256 joints per skin or 1,024 total joints preview limit"
                );
                let mut unique = HashSet::new();
                for &joint in &skin.joints {
                    ensure!(
                        joint < self.document.nodes.len() && unique.insert(joint),
                        "GLB skin has a missing or duplicate joint"
                    );
                    ensure!(
                        selected_set.contains(&joint),
                        "GLB selected skin joints must belong to the selected scene"
                    );
                    ensure!(
                        root_of[joint] == root_of[skin.joints[0]],
                        "GLB skin joints require a common root"
                    );
                }
                if let Some(skeleton) = skin.skeleton {
                    ensure!(
                        skeleton < self.document.nodes.len(),
                        "GLB skin skeleton references a missing node"
                    );
                    for &joint in &skin.joints {
                        let mut ancestor = Some(joint);
                        while ancestor.is_some_and(|v| v != skeleton) {
                            ancestor = parent_of[ancestor.unwrap()];
                        }
                        ensure!(
                            ancestor == Some(skeleton),
                            "GLB skin skeleton must be an ancestor of every joint"
                        );
                    }
                }
                let inverse_bind = if let Some(accessor) = skin.inverse_bind_matrices {
                    let source = self
                        .document
                        .accessors
                        .get(accessor)
                        .context("GLB inverse bind matrices reference a missing accessor")?;
                    ensure!(
                        source.kind == "MAT4"
                            && source.component_type == 5126
                            && !source.normalized
                            && source.count >= skin.joints.len(),
                        "GLB inverse bind matrices require FLOAT MAT4 values for every joint"
                    );
                    let values = self.deformation_values(accessor, 16)?;
                    let mut matrices = Vec::with_capacity(skin.joints.len());
                    for matrix in values.as_chunks::<16>().0.iter().take(skin.joints.len()) {
                        ensure!(
                            matrix[3] == 0.
                                && matrix[7] == 0.
                                && matrix[11] == 0.
                                && matrix[15] == 1.,
                            "GLB inverse bind matrix is not affine"
                        );
                        matrices.push(*matrix);
                    }
                    matrices
                } else {
                    vec![IDENTITY; skin.joints.len()]
                };
                entry.insert(RetainedSkin {
                    joints: skin.joints.clone(),
                    inverse_bind,
                });
            }
            let mesh = &self.meshes[&mesh_index];
            for primitive in &mesh.primitives {
                triangle_count += primitive.indices.len() / 3;
                ensure!(
                    triangle_count <= MAX_MODEL_TRIANGLES,
                    "GLB instances exceed the 100,000 expanded triangle preview limit"
                );
                frame_work += primitive.positions.len()
                    * (1 + primitive.morphs.len() + if node.skin.is_some() { 8 } else { 0 });
                ensure!(
                    frame_work <= MAX_FRAME_WORK,
                    "GLB exceeds the 8,000,000 per-frame deformation work limit"
                );
                if let Some(skin_index) = node.skin {
                    let skin = &skins[&skin_index];
                    let influences = primitive
                        .influences
                        .as_ref()
                        .context("GLB skinned primitive requires JOINTS_0 and WEIGHTS_0")?;
                    ensure!(
                        influences.iter().all(|v| v
                            .joints
                            .iter()
                            .zip(v.weights)
                            .all(|(&joint, _weight)| (joint as usize) < skin.joints.len())),
                        "GLB skin joint index is outside the skin joint list"
                    );
                }
            }
        }
        let clips = self.animation_clips(details)?;
        let metadata = clips.iter().map(|v| v.metadata.clone()).collect();
        let mut result = GlbAnimation {
            nodes: self.document.nodes.clone(),
            transforms,
            hierarchy,
            selected,
            meshes: std::mem::take(&mut self.meshes),
            skins,
            clips,
            metadata,
            retained_bytes: 0,
            triangle_count,
        };
        result.retained_bytes = result.count_retained_bytes();
        ensure!(
            result.retained_bytes <= MAX_RETAINED_BYTES,
            "GLB exceeds the 64 MiB retained deformation preview limit"
        );
        if !result.skins.is_empty() {
            details.push("Linear blend skinning evaluates up to eight joint influences per vertex and authored inverse bind matrices.".into());
        }
        if result.meshes.values().any(|v| !v.weights.is_empty()) {
            details.push("POSITION morph targets apply before skinning; authored normal and tangent morphs do not affect flat inspection shading.".into());
        }
        Ok(result)
    }

    fn decode_mesh(&mut self, index: usize) -> Result<RetainedMesh> {
        let source = &self.document.meshes[index];
        ensure!(
            !source.primitives.is_empty(),
            "GLB selected mesh contains no primitives"
        );
        let mut mesh = RetainedMesh {
            primitives: Vec::new(),
            weights: source
                .weights
                .clone()
                .unwrap_or_else(|| vec![0.; source.primitives[0].targets.len()]),
            appearance: Vec::new(),
        };
        for primitive in &source.primitives {
            (self.check)()?;
            ensure!(
                primitive.mode.unwrap_or(4) == 4,
                "GLB primitive mode is unsupported; only TRIANGLES geometry is rendered"
            );
            let position = primitive.attributes["POSITION"];
            let element_count = primitive
                .indices
                .map_or(self.document.accessors[position].count, |v| {
                    self.document.accessors[v].count
                });
            ensure!(
                element_count >= 3 && element_count.is_multiple_of(3),
                "GLB TRIANGLES requires an element count divisible by three"
            );
            ensure!(
                element_count / 3 <= MAX_MODEL_TRIANGLES.saturating_sub(self.source_triangles),
                "GLB exceeds the 100,000 source triangle preview limit"
            );
            self.source_triangles += element_count / 3;
            let positions = self.positions(position)?;
            let indices = if let Some(index) = primitive.indices {
                self.indices(index, positions.len())?
            } else {
                (0..positions.len()).collect()
            };
            self.retain(
                positions.len() * std::mem::size_of::<Point>()
                    + indices.len() * std::mem::size_of::<usize>(),
            )?;
            let mut morphs = Vec::new();
            for target in &primitive.targets {
                ensure!(
                    target
                        .keys()
                        .all(|v| matches!(v.as_str(), "POSITION" | "NORMAL" | "TANGENT")),
                    "GLB morph target contains an unsupported attribute"
                );
                for (name, &accessor) in target {
                    let source = self
                        .document
                        .accessors
                        .get(accessor)
                        .context("GLB morph target references a missing accessor")?;
                    ensure!(
                        source.kind == "VEC3" && source.count == positions.len(),
                        "GLB morph target accessor must match POSITION count and VEC3 shape"
                    );
                    ensure!(
                        (source.component_type == 5126 && !source.normalized)
                            || (self.quantized && matches!(source.component_type, 5120..=5123)),
                        "Unsupported GLB morph target component type"
                    );
                    if name != "POSITION" {
                        continue;
                    }
                }
                let values = if let Some(&accessor) = target.get("POSITION") {
                    self.deformation_values(accessor, 3)?
                        .as_chunks::<3>()
                        .0
                        .to_vec()
                } else {
                    self.retain(positions.len() * std::mem::size_of::<Point>())?;
                    vec![[0.; 3]; positions.len()]
                };
                morphs.push(values);
            }
            let influences = self.skin_influences(primitive, positions.len())?;
            mesh.appearance.extend(self.primitive_appearance(
                primitive,
                &indices,
                positions.len(),
            )?);
            mesh.primitives.push(RetainedPrimitive {
                positions,
                indices,
                morphs,
                influences,
            });
        }
        Ok(mesh)
    }

    fn skin_influences(
        &mut self,
        primitive: &Primitive,
        count: usize,
    ) -> Result<Option<Vec<Influence>>> {
        for name in primitive.attributes.keys() {
            if name.starts_with("JOINTS_") || name.starts_with("WEIGHTS_") {
                ensure!(
                    matches!(
                        name.as_str(),
                        "JOINTS_0" | "JOINTS_1" | "WEIGHTS_0" | "WEIGHTS_1"
                    ),
                    "GLB skin supports at most eight influences (JOINTS_0/1 and WEIGHTS_0/1)"
                );
            }
        }
        if !primitive.attributes.contains_key("JOINTS_0")
            && !primitive.attributes.contains_key("WEIGHTS_0")
        {
            ensure!(
                !primitive.attributes.contains_key("JOINTS_1")
                    && !primitive.attributes.contains_key("WEIGHTS_1"),
                "GLB skin influence sets must start at zero"
            );
            return Ok(None);
        }
        self.retain(count * std::mem::size_of::<Influence>())?;
        let mut output: Vec<Influence> = (0..count)
            .map(|_| Influence {
                joints: [0; 8],
                weights: [0.; 8],
            })
            .collect();
        for set in 0..2 {
            let joint = primitive.attributes.get(&format!("JOINTS_{set}"));
            let weight = primitive.attributes.get(&format!("WEIGHTS_{set}"));
            if set == 1 && joint.is_none() && weight.is_none() {
                continue;
            }
            let (&joint, &weight) = (
                joint.context("GLB skin weights are missing matching joints")?,
                weight.context("GLB skin joints are missing matching weights")?,
            );
            let j = &self.document.accessors[joint];
            let w = &self.document.accessors[weight];
            ensure!(
                j.kind == "VEC4" && matches!(j.component_type, 5121 | 5123) && !j.normalized,
                "GLB skin JOINTS requires non-normalized unsigned BYTE/SHORT VEC4"
            );
            ensure!(
                w.kind == "VEC4"
                    && ((w.component_type == 5126 && !w.normalized)
                        || (matches!(w.component_type, 5121 | 5123) && w.normalized)),
                "GLB skin WEIGHTS requires FLOAT or normalized unsigned BYTE/SHORT VEC4"
            );
            let joints = self.deformation_values(joint, 4)?;
            let weights = self.deformation_values(weight, 4)?;
            for (i, influence) in output.iter_mut().enumerate() {
                if i.is_multiple_of(256) {
                    (self.check)()?;
                }
                for lane in 0..4 {
                    let weight = weights[i * 4 + lane];
                    ensure!(
                        (0. ..=1.).contains(&weight),
                        "GLB skin weights must be in [0,1]"
                    );
                    influence.joints[set * 4 + lane] = joints[i * 4 + lane] as u16;
                    influence.weights[set * 4 + lane] = weight;
                }
            }
        }
        for (vertex, influence) in output.iter_mut().enumerate() {
            if vertex.is_multiple_of(256) {
                (self.check)()?;
            }
            for first in 0..8 {
                for second in first + 1..8 {
                    ensure!(
                        influence.weights[first] == 0.
                            || influence.weights[second] == 0.
                            || influence.joints[first] != influence.joints[second],
                        "GLB skin repeats a nonzero joint influence for a vertex"
                    );
                }
            }
            let sum: f64 = influence.weights.iter().sum();
            ensure!(
                sum > 1e-12 && (sum - 1.).abs() <= 1e-4,
                "GLB skin weights must have a nonzero sum near one"
            );
            for weight in &mut influence.weights {
                *weight /= sum;
            }
        }
        Ok(Some(output))
    }
}

impl GlbAnimation {
    pub(in crate::model3d) fn clips(&self) -> &[ModelAnimationClip] {
        &self.metadata
    }
    pub(in crate::model3d) fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
    pub(in crate::model3d) fn evaluate(
        &self,
        clip: usize,
        time_seconds: f64,
        check: &impl Fn() -> Result<()>,
    ) -> Result<GlbFrame> {
        check()?;
        ensure!(
            time_seconds.is_finite(),
            "GLB animation time must be finite"
        );
        let clip = self
            .clips
            .get(clip)
            .context("GLB animation clip is unavailable")?;
        self.frame(
            Some((clip, time_seconds.clamp(0., clip.metadata.duration_seconds))),
            check,
        )
    }
    pub(super) fn default_frame(&self, check: &impl Fn() -> Result<()>) -> Result<GlbFrame> {
        self.frame(None, check)
    }
    pub(super) fn take_appearance_triangles(&mut self) -> Vec<TriangleAppearance> {
        let mut triangles = Vec::with_capacity(self.triangle_count);
        for &index in &self.selected {
            if let Some(mesh) = self.nodes[index].mesh {
                triangles.extend_from_slice(&self.meshes[&mesh].appearance);
            }
        }
        // Appearance is shared once on ModelScene; deformation never duplicates
        // immutable per-triangle colors/UVs in its retained source.
        for mesh in self.meshes.values_mut() {
            mesh.appearance = Vec::new();
        }
        self.retained_bytes = self.count_retained_bytes();
        triangles
    }
    fn frame(
        &self,
        animation: Option<(&Clip, f64)>,
        check: &impl Fn() -> Result<()>,
    ) -> Result<GlbFrame> {
        check()?;
        let mut locals = self.transforms.clone();
        let mut animated_nodes: HashMap<usize, Node> = HashMap::new();
        if let Some((clip, time)) = animation {
            for channel in &clip.channels {
                check()?;
                let node = animated_nodes
                    .entry(channel.node)
                    .or_insert_with(|| self.nodes[channel.node].clone());
                channel.apply(node, time)?;
            }
            for (&index, node) in &animated_nodes {
                locals[index] = node_transform(node)?;
            }
        }
        let mut worlds = vec![IDENTITY; self.nodes.len()];
        for &(index, parent) in &self.hierarchy {
            check()?;
            worlds[index] = compose(parent.map_or(IDENTITY, |v| worlds[v]), locals[index])?;
        }
        let mut joint_matrices = HashMap::new();
        for (&index, skin) in &self.skins {
            let mut matrices = Vec::with_capacity(skin.joints.len());
            for (&joint, &inverse) in skin.joints.iter().zip(&skin.inverse_bind) {
                check()?;
                matrices.push(compose(worlds[joint], inverse)?);
            }
            joint_matrices.insert(index, matrices);
        }
        let mut frame = GlbFrame {
            triangles: Vec::with_capacity(self.triangle_count),
            reverse_winding: Vec::with_capacity(self.triangle_count),
        };
        for &index in &self.selected {
            check()?;
            let node = animated_nodes.get(&index).unwrap_or(&self.nodes[index]);
            let Some(mesh_index) = node.mesh else {
                continue;
            };
            let mesh = &self.meshes[&mesh_index];
            let weights = node.weights.as_deref().unwrap_or(&mesh.weights);
            validate_weights(weights, mesh.weights.len())?;
            let world = worlds[index];
            // Skinning replaces mesh-node placement. Its triangle winding is
            // determined by the posed vertices, not the ignored mesh transform.
            let reverse = node.skin.is_none() && determinant(world) < 0.;
            for primitive in &mesh.primitives {
                let mut positions = Vec::with_capacity(primitive.positions.len());
                for (vertex, &base) in primitive.positions.iter().enumerate() {
                    if vertex.is_multiple_of(256) {
                        check()?;
                    }
                    let mut position = base;
                    for (target, &weight) in primitive.morphs.iter().zip(weights) {
                        for axis in 0..3 {
                            position[axis] += target[vertex][axis] * weight;
                        }
                    }
                    let point = if let Some(skin) = node.skin {
                        let influence = &primitive.influences.as_ref().unwrap()[vertex];
                        let matrices = &joint_matrices[&skin];
                        let mut point = [0.; 3];
                        for (&joint, &weight) in influence.joints.iter().zip(&influence.weights) {
                            if weight == 0. {
                                continue;
                            }
                            let transformed = point_raw(matrices[joint as usize], position);
                            for axis in 0..3 {
                                point[axis] += transformed[axis] * weight;
                            }
                        }
                        valid_point([1000. * point[0], -1000. * point[2], 1000. * point[1]])?
                    } else {
                        transform_point(world, position)?
                    };
                    positions.push(point);
                }
                for (i, indices) in primitive.indices.as_chunks::<3>().0.iter().enumerate() {
                    if i.is_multiple_of(256) {
                        check()?;
                    }
                    frame.triangles.push(indices.map(|v| positions[v]));
                    frame.reverse_winding.push(reverse);
                }
            }
        }
        check()?;
        Ok(frame)
    }
    fn count_retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.meshes.capacity() * (std::mem::size_of::<(usize, RetainedMesh)>() + 1)
            + self.skins.capacity() * (std::mem::size_of::<(usize, RetainedSkin)>() + 1)
            + self.nodes.capacity() * std::mem::size_of::<Node>()
            + self
                .nodes
                .iter()
                .map(|v| {
                    v.children.capacity() * std::mem::size_of::<usize>()
                        + v.weights.as_ref().map_or(0, |v| v.capacity() * 8)
                })
                .sum::<usize>()
            + self.transforms.capacity() * std::mem::size_of::<Matrix>()
            + self.hierarchy.capacity() * std::mem::size_of::<(usize, Option<usize>)>()
            + self.selected.capacity() * std::mem::size_of::<usize>()
            + self
                .meshes
                .values()
                .map(|v| {
                    v.primitives.capacity() * std::mem::size_of::<RetainedPrimitive>()
                        + v.weights.capacity() * 8
                        + v.appearance.capacity() * std::mem::size_of::<TriangleAppearance>()
                        + v.primitives
                            .iter()
                            .map(|p| {
                                p.positions.capacity() * std::mem::size_of::<Point>()
                                    + p.indices.capacity() * std::mem::size_of::<usize>()
                                    + p.morphs.capacity() * std::mem::size_of::<Vec<Point>>()
                                    + p.morphs
                                        .iter()
                                        .map(|m| m.capacity() * std::mem::size_of::<Point>())
                                        .sum::<usize>()
                                    + p.influences.as_ref().map_or(0, |i| {
                                        i.capacity() * std::mem::size_of::<Influence>()
                                    })
                            })
                            .sum::<usize>()
                })
                .sum::<usize>()
            + self
                .skins
                .values()
                .map(|s| {
                    s.joints.capacity() * std::mem::size_of::<usize>()
                        + s.inverse_bind.capacity() * std::mem::size_of::<Matrix>()
                })
                .sum::<usize>()
            + self.clips.iter().map(Clip::retained_bytes).sum::<usize>()
            + self.metadata.capacity() * std::mem::size_of::<ModelAnimationClip>()
            + self
                .metadata
                .iter()
                .map(|c| c.name.capacity())
                .sum::<usize>()
    }
}

fn point_raw(matrix: Matrix, point: Point) -> Point {
    std::array::from_fn(|i| {
        matrix[i] * point[0] + matrix[4 + i] * point[1] + matrix[8 + i] * point[2] + matrix[12 + i]
    })
}
fn determinant(m: Matrix) -> f64 {
    m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2])
        + m[8] * (m[1] * m[6] - m[5] * m[2])
}

#[cfg(test)]
mod tests;
