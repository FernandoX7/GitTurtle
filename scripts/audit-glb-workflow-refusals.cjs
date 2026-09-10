#!/usr/bin/env node
// Independent narrow audit of known captured corpus skin refusals. Uses only
// supplied GLBs and the official meshoptimizer decoder; no resource resolver.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const [decoderPath, ...inputs] = process.argv.slice(2);
if (!inputs.length) throw new Error('usage: audit-glb-workflow-refusals.cjs MESHOPT_DECODER_JS GLB...');
const decoder = require(path.resolve(decoderPath));
const hash = bytes => crypto.createHash('sha256').update(bytes).digest('hex');

async function main() {
  await decoder.ready;
  for (const input of inputs) {
    const bytes = fs.readFileSync(input), length = bytes.readUInt32LE(12);
    const doc = JSON.parse(bytes.subarray(20, 20 + length)), binary = bytes.subarray(28 + length);
    const views = new Map();
    function accessor(index) {
      const a = doc.accessors[index], v = doc.bufferViews[a.bufferView];
      if (a.sparse) throw new Error('Audit requires nonsparse known corpus accessors');
      if (!views.has(a.bufferView)) {
        const ext = v.extensions?.EXT_meshopt_compression;
        let data;
        if (ext) {
          if (ext.buffer !== 0) throw new Error('Noncaptured compressed bytes refused');
          data = new Uint8Array(ext.count * ext.byteStride);
          decoder.decodeGltfBuffer(data, ext.count, ext.byteStride,
            binary.subarray(ext.byteOffset, ext.byteOffset + ext.byteLength), ext.mode, ext.filter);
        } else {
          if (v.buffer !== 0) throw new Error('Noncaptured accessor bytes refused');
          data = binary.subarray(v.byteOffset ?? 0, (v.byteOffset ?? 0) + v.byteLength);
        }
        views.set(a.bufferView, data);
      }
      const data = views.get(a.bufferView), reader = new DataView(data.buffer, data.byteOffset, data.byteLength);
      const components = {SCALAR: 1, VEC4: 4, MAT4: 16}[a.type];
      const size = {5121: 1, 5123: 2, 5126: 4}[a.componentType];
      if (!components || !size) throw new Error('Unsupported audit accessor');
      const rows = [];
      for (let i = 0; i < a.count; i++) {
        const row = [];
        for (let c = 0; c < components; c++) {
          const offset = (a.byteOffset ?? 0) + i * (v.byteStride ?? components * size) + c * size;
          const value = a.componentType === 5121 ? reader.getUint8(offset)
            : a.componentType === 5123 ? reader.getUint16(offset, true) : reader.getFloat32(offset, true);
          row.push(a.normalized ? value / (a.componentType === 5121 ? 255 : 65535) : value);
        }
        rows.push(row);
      }
      return {descriptor: a, rows};
    }
    const parents = new Map();
    (doc.nodes ?? []).forEach((node, index) => (node.children ?? []).forEach(child => parents.set(child, index)));
    const defects = [];
    for (const [index, skin] of (doc.skins ?? []).entries()) {
      if (skin.skeleton !== undefined) {
        const outside = skin.joints.filter(joint => {
          const seen = new Set();
          while (!seen.has(joint)) {
            if (joint === skin.skeleton) return false;
            seen.add(joint);
            if (!parents.has(joint)) return true;
            joint = parents.get(joint);
          }
          throw new Error('Cycle in known corpus hierarchy');
        });
        if (outside.length) defects.push({skin: index, kind: 'skeleton-is-not-common-ancestor', skeleton: skin.skeleton, joints_outside: outside});
      }
      if (skin.inverseBindMatrices !== undefined) {
        const rows = accessor(skin.inverseBindMatrices).rows;
        const changed = rows.map((matrix, joint) => ({joint, fourth_row: [matrix[3], matrix[7], matrix[11], matrix[15]]}))
          .filter(row => row.fourth_row.some((value, i) => value !== (i === 3 ? 1 : 0)));
        if (changed.length) defects.push({skin: index, kind: 'noncanonical-inverse-bind-fourth-row', affected_matrices: changed.length,
          maximum_absolute_error: Math.max(...changed.flatMap(row => row.fourth_row.map((value, i) => Math.abs(value - (i === 3 ? 1 : 0))))), examples: changed.slice(0, 3)});
      }
    }
    for (const [meshIndex, mesh] of (doc.meshes ?? []).entries()) {
      for (const [primitiveIndex, primitive] of mesh.primitives.entries()) {
        if (primitive.targets?.length > 8) defects.push({mesh: meshIndex, primitive: primitiveIndex, kind: 'morph-target-limit', targets: primitive.targets.length});
        if (primitive.attributes.WEIGHTS_0 === undefined) continue;
        const weights = [accessor(primitive.attributes.WEIGHTS_0)];
        if (primitive.attributes.WEIGHTS_1 !== undefined) weights.push(accessor(primitive.attributes.WEIGHTS_1));
        const invalid = [];
        for (let vertex = 0; vertex < weights[0].rows.length; vertex++) {
          const values = weights.flatMap(weight => weight.rows[vertex]);
          const sum = values.reduce((total, value) => total + value, 0);
          if (Math.abs(sum - 1) > 1e-4) invalid.push({vertex, weights: values, sum});
        }
        if (invalid.length) defects.push({mesh: meshIndex, primitive: primitiveIndex, kind: 'weight-sum',
          components: weights.map(weight => weight.descriptor.componentType), affected_vertices: invalid.length, examples: invalid.slice(0, 8)});
      }
    }
    console.log(JSON.stringify({name: path.basename(input), sha256: hash(bytes), defects}));
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
