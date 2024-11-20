/////////////// Based on K.jpg's Re-oriented 8-Point BCC Noise (OpenSimplex2S) ////////////////
////////////////////// Output: Value Array //////////////////////

// Borrowed from Stefan Gustavson's noise code
fn permute(t: vec4<f32>) -> vec4<f32> {
	return t * (t * 34. + 133.);
}

// Gradient set is a normalized expanded rhombic dodecahedron
fn grad(hash: f32) -> vec3<f32> {
	let cube: vec3<f32> = ((floor(hash / vec3<f32>(1., 2., 4.))) % (2.)) * 2. - 1.;
	var cuboct: vec3<f32> = cube;
	cuboct[i32(hash / 16.)] = 0.;
	let grtype: f32 = ((floor(hash / 8.)) % (2.));
	let rhomb: vec3<f32> = (1. - grtype) * cube + grtype * (cuboct + cross(cube, cuboct));
	var grad: vec3<f32> = cuboct * 1.2247449 + rhomb;
	grad = grad * ((1. - 0.04294244 * grtype) * 3.5946317);
	return grad;
}

// BCC lattice split up into 2 cube lattices
fn openSimplex2SValuePart(X: vec3<f32>, seed: f32) -> f32 {
	let b: vec3<f32> = floor(X);
	let i4: vec4<f32> = vec4<f32>(X - b, 2.5);
	let v1: vec3<f32> = b + floor(dot(i4, vec4<f32>(0.25)));
	let v2: vec3<f32> = b + vec3<f32>(1., 0., 0.) + vec3<f32>(-1., 1., 1.) * floor(dot(i4, vec4<f32>(-0.25, 0.25, 0.25, 0.35)));
	let v3: vec3<f32> = b + vec3<f32>(0., 1., 0.) + vec3<f32>(1., -1., 1.) * floor(dot(i4, vec4<f32>(0.25, -0.25, 0.25, 0.35)));
	let v4: vec3<f32> = b + vec3<f32>(0., 0., 1.) + vec3<f32>(1., 1., -1.) * floor(dot(i4, vec4<f32>(0.25, 0.25, -0.25, 0.35)));
	var hashes: vec4<f32> = permute(((vec4<f32>(v1.x, v2.x, v3.x, v4.x) + seed) % (289.)));
	hashes = permute(((hashes + vec4<f32>(v1.y, v2.y, v3.y, v4.y)) % (289.)));
	hashes = ((permute(((hashes + vec4<f32>(v1.z, v2.z, v3.z, v4.z)) % (289.)))) % (48.));
	let d1: vec3<f32> = X - v1;
	let d2: vec3<f32> = X - v2;
	let d3: vec3<f32> = X - v3;
	let d4: vec3<f32> = X - v4;
	let a: vec4<f32> = max(vec4<f32>(0.75) - vec4<f32>(dot(d1, d1), dot(d2, d2), dot(d3, d3), dot(d4, d4)), vec4<f32>(0.));
	let aa: vec4<f32> = a * a;
	let aaaa: vec4<f32> = aa * aa;
	let g1: vec3<f32> = grad(hashes.x);
	let g2: vec3<f32> = grad(hashes.y);
	let g3: vec3<f32> = grad(hashes.z);
	let g4: vec3<f32> = grad(hashes.w);
	let extrapolations: vec4<f32> = vec4<f32>(dot(d1, g1), dot(d2, g2), dot(d3, g3), dot(d4, g4));
	let derivative: vec3<f32> = -8. * mat4x3<f32>(d1, d2, d3, d4) * (aa * a * extrapolations) + mat4x3<f32>(g1, g2, g3, g4) * aaaa;
	return dot(aaaa, extrapolations);
}

// Use this if you don't want Z to look different from X and Y
fn openSimplex2SValue_Conventional(X: vec3<f32>, seed: f32) -> f32 {
				let newX = dot(X, vec3<f32>(2.0/3.0)) - X;
				var result = openSimplex2SValuePart(newX, seed) + openSimplex2SValuePart(newX + 144.5, seed);
				return result;
}

// Use this if you want to show X and Y in a plane, then use Z for time, vertical, etc.
fn openSimplex2SValue_ImproveXY(X: vec3<f32>, seed: f32) -> f32 {
				var orthonormalMap = mat3x3<f32>(
								0.788675134594813, -0.211324865405187, -0.577350269189626,
								-0.211324865405187, 0.788675134594813, -0.577350269189626,
								0.577350269189626, 0.577350269189626, 0.577350269189626);
				let newX = orthonormalMap * X;
				var result = openSimplex2SValuePart(newX, seed) + openSimplex2SValuePart(newX + 144.5, seed);
				return result;
}

// Function to generate noise with octaves and frequency
fn generateNoise(position: vec3<f32>, frequency: f32, lacunarity: f32, persistence: f32, octaves: u32, useConventional: u32, seed: f32) -> f32 {
				var useConventionalBool = useConventional != 0u;
				var freq = frequency;
				var amplitude = 1.0;
				var totalValue = 0.0;
				for (var i: u32 = 0u; i < octaves; i = i + 1u) {
								var scaledPosition = position * freq;
								var noiseValue: f32;
								if (useConventionalBool) {
												noiseValue = openSimplex2SValue_Conventional(scaledPosition, seed);
								} else {
												noiseValue = openSimplex2SValue_ImproveXY(scaledPosition, seed);
								}
								totalValue += noiseValue * amplitude;
								freq *= lacunarity;
								amplitude *= persistence;
				}
				return totalValue;
}

fn toRGBA(value: f32) -> vec4<u32> {
    let clamped = clamp(value, -1.0, 1.0);  // Ensure value is in [-1, 1]
    let intensity_f32 = ((clamped + 1.0) / 2.0) * 255.0;
    let intensity = u32(intensity_f32);
    return vec4<u32>(intensity, intensity, intensity, 255);
}

@group(0) @binding(0) var<uniform> seed: f32;
@group(0) @binding(1) var<uniform> start: vec3<f32>;
@group(0) @binding(2) var<uniform> next: vec3<f32>;
@group(0) @binding(3) var<uniform> frequency: f32;
@group(0) @binding(4) var<uniform> lacunarity: f32;
@group(0) @binding(5) var<uniform> persistence: f32;
@group(0) @binding(6) var<uniform> octaves: u32;
@group(0) @binding(7) var<uniform> useConventional: u32;
@group(0) @binding(8) var<uniform> target_dims: vec3<u32>;
@group(0) @binding(9) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    // Check if we're within target dimensions
    if (id.x >= target_dims.x || id.y >= target_dims.y || id.z >= target_dims.z) {
        return;
    }

    // Calculate position based on target dimensions
    let t = vec3<f32>(
        f32(id.x) / f32(target_dims.x),
        f32(id.y) / f32(target_dims.y),
        f32(id.z) / f32(target_dims.z)
    );

    // Convert to WebGPU coordinates before interpolation
    let start_webgpu = vec3<f32>(start.x, -start.y, -start.z);
    let next_webgpu = vec3<f32>(next.x, -next.y, -next.z);
    let position_webgpu = mix(start_webgpu, next_webgpu, t);

    // Calculate index in the output array
    let base_index = (id.x + id.y * target_dims.x + id.z * target_dims.x * target_dims.y) * 4;
    let value = generateNoise(position_webgpu, frequency, lacunarity, persistence, octaves, useConventional, seed);
    let rgba = toRGBA(value);
    output[base_index] = rgba.x;
    output[base_index + 1] = rgba.y;
    output[base_index + 2] = rgba.z;
    output[base_index + 3] = rgba.w;
}

//////////////////////////////// End noise code ////////////////////////////////
