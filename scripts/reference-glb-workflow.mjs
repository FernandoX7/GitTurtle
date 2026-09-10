#!/usr/bin/env node
// Independent validation only: maintained Three.js GLTFLoader/AnimationMixer/
// SkinnedMesh evaluate captured bytes; sharp decodes embedded PNG/JPEG. No UI,
// external resources, package install, renderer fork or production decoder.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';

const args = process.argv.slice(2);
const geometryOnly = args.includes('--geometry-only');
const normalizeRotations = args.includes('--normalize-rotation-keys');
for (const option of ['--geometry-only', '--normalize-rotation-keys']) {
  const index = args.indexOf(option);
  if (index !== -1) args.splice(index, 1);
}
const [modules, meshoptPath, outputDirectory, ...sources] = args;
if (!sources.length) throw new Error('usage: reference-glb-workflow.mjs [--geometry-only] [--normalize-rotation-keys] NODE_MODULES MESHOPT_DECODER_JS OUTPUT_DIRECTORY GLB...');
const require = createRequire(import.meta.url);
const THREE = await import(pathToFileURL(path.resolve(modules, 'three/build/three.module.js')));
const { GLTFLoader } = await import(pathToFileURL(path.resolve(modules, 'three/examples/jsm/loaders/GLTFLoader.js')));
const sharp = require(path.resolve(modules, 'sharp'));
const MeshoptDecoder = require(path.resolve(meshoptPath));
await MeshoptDecoder.ready;
const sha = value => crypto.createHash('sha256').update(value).digest('hex');
const packageInfo = name => JSON.parse(fs.readFileSync(path.resolve(modules, name, 'package.json')));
const identity = {three: packageInfo('three').version, sharp: packageInfo('sharp').version,
  node: process.version, loader_sha256: sha(fs.readFileSync(path.resolve(modules, 'three/examples/jsm/loaders/GLTFLoader.js'))),
  three_core_sha256: sha(fs.readFileSync(path.resolve(modules, 'three/build/three.core.js'))),
  meshopt_decoder_sha256: sha(fs.readFileSync(meshoptPath)), script_sha256: sha(fs.readFileSync(import.meta.filename))};

// ImageBitmapLoader creates blob URLs from the captured BIN chunk. A strict
// process-wide fetch gate makes any accidentally requested asset URL fail closed.
const originalFetch = globalThis.fetch;
let blobReads = 0;
globalThis.fetch = (url, options) => {
  if (!String(url).startsWith('blob:nodedata:')) throw new Error('External/data resource fetching is forbidden');
  blobReads++;
  return originalFetch(url, options);
};
globalThis.self = globalThis;
globalThis.createImageBitmap = async blob => {
  if (!['image/png', 'image/jpeg'].includes(blob.type)) throw new Error(`Reference image format unsupported: ${blob.type}`);
  const bytes = Buffer.from(await blob.arrayBuffer());
  const {data, info} = await sharp(bytes, {limitInputPixels: 16 * 1024 * 1024}).ensureAlpha().raw().toBuffer({resolveWithObject: true});
  return {data, width: info.width, height: info.height, close() {}};
};

function unpack(bytes) {
  if (bytes.readUInt32LE(0) !== 0x46546c67 || bytes.readUInt32LE(4) !== 2 || bytes.readUInt32LE(8) !== bytes.length) throw new Error('Expected complete GLB 2.0');
  const length = bytes.readUInt32LE(12);
  const doc = JSON.parse(bytes.subarray(20, 20 + length).toString());
  const binOffset = 28 + length;
  return {doc, binary: bytes.subarray(binOffset)};
}

function repack(doc, binary) {
  let json = Buffer.from(JSON.stringify(doc));
  json = Buffer.concat([json, Buffer.alloc((4 - json.length % 4) % 4, 32)]);
  const head = Buffer.alloc(20), binHead = Buffer.alloc(8);
  head.writeUInt32LE(0x46546c67); head.writeUInt32LE(2, 4);
  head.writeUInt32LE(28 + json.length + binary.length, 8);
  head.writeUInt32LE(json.length, 12); head.writeUInt32LE(0x4e4f534a, 16);
  binHead.writeUInt32LE(binary.length); binHead.writeUInt32LE(0x004e4942, 4);
  return Buffer.concat([head, json, binHead, binary]);
}

function triangles(scene) {
  scene.updateMatrixWorld(true);
  scene.traverse(node => { if (node.isSkinnedMesh) node.skeleton.update(); });
  const points = [];
  scene.traverse(node => {
    if (!node.isMesh) return;
    const geometry = node.geometry;
    const index = geometry.index;
    const count = index ? index.count : geometry.attributes.position.count;
    if (count % 3) throw new Error('Nontriangle reference primitive');
    for (let i = 0; i < count; i++) {
      const point = node.getVertexPosition(index ? index.getX(i) : i, new THREE.Vector3()).applyMatrix4(node.matrixWorld);
      points.push(point.x * 1000, -point.z * 1000, point.y * 1000);
    }
  });
  const binary = Buffer.alloc(points.length * 8);
  points.forEach((value, index) => binary.writeDoubleLE(value, index * 8));
  const minimum = [Infinity, Infinity, Infinity], maximum = [-Infinity, -Infinity, -Infinity];
  points.forEach((value, i) => { minimum[i % 3] = Math.min(minimum[i % 3], value); maximum[i % 3] = Math.max(maximum[i % 3], value); });
  return {binary, count: points.length / 9, minimum, maximum};
}

function materialInfo(scene) {
  const result = [];
  scene.traverse(node => {
    if (!node.isMesh) return;
    for (const material of Array.isArray(node.material) ? node.material : [node.material]) {
      const map = material.map;
      result.push({type: material.type, linear_base_color: material.color.toArray(), opacity: material.opacity,
        transparent: material.transparent, alpha_test: material.alphaTest, side: material.side,
        vertex_colors: material.vertexColors,
        texture: map ? {width: map.image.width, height: map.image.height,
          rgba_sha256: map.image.data ? sha(map.image.data) : null, color_space: map.colorSpace,
          wrap_s: map.wrapS, wrap_t: map.wrapT, min_filter: map.minFilter, mag_filter: map.magFilter, flip_y: map.flipY} : null});
    }
  });
  return result;
}

function unlitSamples(scene, geometry, edge = 720) {
  // Raycaster provides independent triangle intersection, winding and UVs. We
  // evaluate unlit nearest texels in linear working color space and composite
  // over the product's declared background. No PBR agreement is claimed here.
  const min = geometry.minimum, max = geometry.maximum;
  const span = Math.hypot(...max.map((v, i) => v - min[i])) * 1.12 / 1000;
  const center = min.map((v, i) => (v + max[i]) / 2000);
  const result = [];
  for (const [px, py] of [[256, 256], [464, 256], [256, 464], [464, 464], [320, 300], [400, 350]]) {
    const origin = new THREE.Vector3(center[0] + ((px + .5) / edge - .5) * span,
      center[2] + (.5 - (py + .5) / edge) * span, -center[1] + span * 2 + 1);
    const hits = new THREE.Raycaster(origin, new THREE.Vector3(0, 0, -1)).intersectObject(scene, true);
    let color = new THREE.Color(28 / 255, 34 / 255, 42 / 255).convertSRGBToLinear();
    let evaluated = false, unsupported = null;
    const unique = hits.filter((hit, index) => !hits.slice(0, index).some(prior => prior.object === hit.object && Math.abs(prior.distance - hit.distance) < 1e-9));
    for (const hit of unique.reverse()) {
      const material = Array.isArray(hit.object.material) ? hit.object.material[hit.face.materialIndex] : hit.object.material;
      if (!material.isMeshBasicMaterial) { unsupported = 'Reference pixel evaluation is limited to KHR_materials_unlit'; break; }
      let foreground = material.color.clone(), alpha = material.opacity;
      const attr = hit.object.geometry.attributes.color;
      if (attr && material.vertexColors) {
        const local = hit.object.worldToLocal(hit.point.clone());
        const points = [hit.face.a, hit.face.b, hit.face.c].map(i => hit.object.getVertexPosition(i, new THREE.Vector3()));
        const bary = THREE.Triangle.getBarycoord(local, ...points, new THREE.Vector3()).toArray();
        const vertex = [0, 0, 0, 0];
        [hit.face.a, hit.face.b, hit.face.c].forEach((i, n) => {
          [attr.getX(i), attr.getY(i), attr.getZ(i), attr.itemSize === 4 ? attr.getW(i) : 1].forEach((v, axis) => vertex[axis] += bary[n] * v);
        });
        foreground.multiply(new THREE.Color(...vertex.slice(0, 3)));
        alpha *= vertex[3];
      }
      if (material.map) {
        const map = material.map;
        if (map.magFilter !== THREE.NearestFilter || !map.image.data) { unsupported = 'Reference pixel evaluation requires captured nearest-filter texture'; break; }
        map.updateMatrix();
        const uv = map.transformUv(hit.uv.clone());
        const x = Math.min(map.image.width - 1, Math.floor(uv.x * map.image.width));
        const y = Math.min(map.image.height - 1, Math.floor(uv.y * map.image.height));
        const rgba = Array.from(map.image.data.subarray((y * map.image.width + x) * 4, (y * map.image.width + x) * 4 + 4));
        foreground.multiply(new THREE.Color(...rgba.slice(0, 3).map(v => v / 255)).convertSRGBToLinear());
        alpha *= rgba[3] / 255;
      }
      if (alpha < material.alphaTest) continue;
      if (!material.transparent) alpha = 1;
      color.multiplyScalar(1 - alpha).add(foreground.multiplyScalar(alpha));
      evaluated = true;
    }
    color.convertLinearToSRGB();
    result.push({pixel: [px, py], rgba: color.toArray().map(v => Math.round(Math.max(0, Math.min(1, v)) * 255)).concat(255), evaluated, unsupported});
  }
  return result;
}

fs.mkdirSync(outputDirectory, {recursive: true});
fs.writeFileSync(path.join(outputDirectory, 'reference-runtime.json'), JSON.stringify(identity, null, 2) + '\n');
for (const [ordinal, source] of sources.entries()) {
  const row = {ordinal, name: path.basename(source), mode: geometryOnly ? 'geometry-only-in-memory-appearance-removal' : 'complete-captured-asset',
    rotation_policy: normalizeRotations ? 'Three.Quaternion.normalize applied to rotation keys before Three interpolation; source bytes untouched' : 'unmodified Three.js loader and interpolation'};
  try {
    const bytes = fs.readFileSync(source);
    row.sha256 = sha(bytes);
    const {doc, binary} = unpack(bytes);
    // Reject active buffer URLs even in geometry-only mode; unused meshopt
    // fallback buffer descriptors are never read by the official decoder.
    for (const [i, buffer] of (doc.buffers ?? []).entries()) {
      if (buffer.uri && !buffer.extensions?.EXT_meshopt_compression?.fallback) throw new Error(`External buffer ${i} refused`);
    }
    if (geometryOnly) {
      delete doc.materials; delete doc.textures; delete doc.images; delete doc.samplers;
      for (const mesh of doc.meshes ?? []) for (const primitive of mesh.primitives) delete primitive.material;
      const appearance = name => name.startsWith('KHR_materials_') || ['KHR_texture_basisu', 'KHR_texture_transform', 'EXT_texture_webp'].includes(name);
      doc.extensionsRequired = (doc.extensionsRequired ?? []).filter(name => !appearance(name));
      doc.extensionsUsed = (doc.extensionsUsed ?? []).filter(name => !appearance(name));
    } else {
      for (const image of doc.images ?? []) {
        if (image.uri) throw new Error('External/data image refused before GLTFLoader');
        const view = doc.bufferViews[image.bufferView];
        if (!view || view.buffer !== 0) throw new Error('Image lacks captured BIN view');
        const data = binary.subarray(view.byteOffset ?? 0, (view.byteOffset ?? 0) + view.byteLength);
        const metadata = await sharp(data, {limitInputPixels: 16 * 1024 * 1024}).metadata();
        if (image.mimeType !== `image/${metadata.format}`) throw new Error('Malformed image: declared MIME differs from captured codec');
      }
    }
    const supplied = geometryOnly ? repack(doc, binary) : bytes;
    const loader = new GLTFLoader().setMeshoptDecoder(MeshoptDecoder);
    const parsed = await loader.parseAsync(supplied.buffer.slice(supplied.byteOffset, supplied.byteOffset + supplied.byteLength), '');
    if (normalizeRotations) {
      // Separate diagnostic run: quantized unit quaternions acquire length
      // error, while stock Three.js preserves that error in linear tracks.
      // Use the maintained Quaternion normalizer, never a copied interpolator.
      // Preserve untouched reference results to make this adjustment reviewable.
      row.maximum_key_norm_error = 0;
      for (const clip of parsed.animations) for (const track of clip.tracks) {
        if (!(track instanceof THREE.QuaternionKeyframeTrack)) continue;
        const cubic = Boolean(track.createInterpolant.isInterpolantFactoryMethodGLTFCubicSpline);
        const values = Float64Array.from(track.values), stride = cubic ? 12 : 4;
        for (let offset = cubic ? 4 : 0; offset < values.length; offset += stride) {
          const quaternion = new THREE.Quaternion().fromArray(values, offset);
          row.maximum_key_norm_error = Math.max(row.maximum_key_norm_error, Math.abs(quaternion.length() - 1));
          quaternion.normalize().toArray(values, offset);
        }
        track.values = values;
      }
    }
    const samples = [];
    const capture = (label, clip, time) => {
      const geometry = triangles(parsed.scene);
      if (!geometry.count) throw new Error('Selected scene has no triangles');
      const name = `${String(ordinal).padStart(4, '0')}-${label}.triangles.bin`;
      fs.writeFileSync(path.join(outputDirectory, name), geometry.binary);
      samples.push({clip, time, triangles: geometry.count, minimum: geometry.minimum, maximum: geometry.maximum,
        triangle_file: name, triangles_sha256: sha(geometry.binary),
        appearance_samples: geometryOnly ? [] : unlitSamples(parsed.scene, geometry)});
    };
    capture('default', null, null);
    row.materials = geometryOnly ? [] : materialInfo(parsed.scene);
    row.clips = parsed.animations.map(c => ({name: c.name, duration: c.duration, tracks: c.tracks.map(t => ({name: t.name, keys: t.times.length}))}));
    for (const [clipIndex, clip] of parsed.animations.entries()) {
      const times = [...new Set([0, .25, .5, 1, 1.5, 2, clip.duration * .25, clip.duration * .5, clip.duration * .75, clip.duration].map(t => Math.min(clip.duration, t)))].sort((a, b) => a - b);
      for (const time of times) {
        const mixer = new THREE.AnimationMixer(parsed.scene);
        const action = mixer.clipAction(clip);
        action.setLoop(THREE.LoopOnce, 1); action.clampWhenFinished = true; action.play();
        mixer.setTime(time);
        capture(`clip${clipIndex}-t${time}`, clipIndex, time);
        mixer.stopAllAction(); mixer.uncacheRoot(parsed.scene);
      }
    }
    row.samples = samples;
    row.ok = true;
  } catch (error) {
    row.ok = false;
    row.error = error.stack;
  }
  console.log(JSON.stringify(row));
}
fs.writeFileSync(path.join(outputDirectory, 'resource-policy.json'), JSON.stringify({blob_reads: blobReads, external_reads: 0, policy: 'Only captured BIN blob URLs; strict global fetch gate; no asset filesystem resolver'}, null, 2) + '\n');
