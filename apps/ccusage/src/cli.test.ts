import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { describe, it, mock } from 'node:test';
import {
	createNativeSpawner,
	ensureNativeBinaryExecutable,
	isMainModule,
	resolveCliRuntime,
	resolveNativeBinary,
} from './cli.js';

void describe(resolveCliRuntime.name, () => {
	void it('resolves the native package binary for the current supported platform', () => {
		const actual = resolveNativeBinary({
			arch: 'arm64',
			platform: 'darwin',
			resolvePath: (id) => {
				assert.equal(id, '@ccusage/ccusage-darwin-arm64/bin/ccusage');
				return '/native/bin/ccusage';
			},
		});

		assert.equal(actual, '/native/bin/ccusage');
	});

	for (const platform of ['linux', 'darwin']) {
		for (const arch of ['x64', 'arm64']) {
			void it(`resolves ${platform}-${arch}`, () => {
				assert.equal(
					resolveNativeBinary({ arch, platform, resolvePath: (id) => id }),
					`@ccusage/ccusage-${platform}-${arch}/bin/ccusage`,
				);
			});
		}
	}

	for (const [platform, arch] of [
		['win32', 'x64'],
		['win32', 'arm64'],
		['linux', 'ia32'],
	]) {
		void it(`rejects unsupported ${platform}-${arch} without resolving a package`, () => {
			const resolvePath = mock.fn((id: string) => id);
			assert.equal(resolveNativeBinary({ arch, platform, resolvePath }), undefined);
			assert.equal(resolvePath.mock.callCount(), 0);
			assert.deepEqual(resolveCliRuntime({ arch, platform, argv: [], nativeBinaryPath: null }), {
				errorMessage: `ccusage does not support ${platform}-${arch}. Supported platforms are Linux and macOS (x64 and arm64).\n`,
			});
		});
	}

	void it('prefers the matching native package binary when it is available', () => {
		assert.deepEqual(
			resolveCliRuntime({
				argv: ['daily'],
				nativeBinaryPath: '/app/node_modules/@ccusage/ccusage-darwin-arm64/bin/ccusage',
			}),
			{
				args: ['daily'],
				command: '/app/node_modules/@ccusage/ccusage-darwin-arm64/bin/ccusage',
			},
		);
	});

	void it('fails when the native package binary is unavailable', () => {
		assert.deepEqual(
			resolveCliRuntime({
				arch: 'arm64',
				argv: ['daily'],
				nativeBinaryPath: null,
				platform: 'darwin',
			}),
			{
				errorMessage:
					'ccusage native binary is not available for darwin-arm64. Reinstall ccusage so optional native dependencies are installed.\n',
			},
		);
	});

	void it('repairs a native binary that was extracted without executable bits', () => {
		const chmodPath = mock.fn();

		assert.equal(
			ensureNativeBinaryExecutable({
				binaryPath: '/native/bin/ccusage',
				chmodPath,
				statPath: () => ({ mode: 0o644 }),
			}),
			undefined,
		);
		assert.deepEqual(
			chmodPath.mock.calls.map((call) => call.arguments),
			[['/native/bin/ccusage', 0o755]],
		);
	});

	void it('does not chmod an already executable native binary', () => {
		const chmodPath = mock.fn();

		assert.equal(
			ensureNativeBinaryExecutable({
				binaryPath: '/native/bin/ccusage',
				chmodPath,
				statPath: () => ({ mode: 0o755 }),
			}),
			undefined,
		);
		assert.equal(chmodPath.mock.callCount(), 0);
	});

	void it('treats package bin symlinks as the main module entry point', () => {
		const actual = isMainModule({
			argvEntry: '/project/node_modules/.bin/ccusage',
			moduleUrl: 'file:///project/node_modules/ccusage/src/cli.js',
			realpathPath: (path) =>
				path === '/project/node_modules/.bin/ccusage'
					? '/project/node_modules/ccusage/src/cli.js'
					: path,
		});

		assert.equal(actual, true);
	});
});

void describe(createNativeSpawner.name, () => {
	void it('forwards every supported launcher signal delivery', async () => {
		const signalSource = new EventEmitter();
		const kill = mock.fn(() => true);
		const child = /** @type {import('node:child_process').ChildProcess} */ (
			/** @type {unknown} */ (Object.assign(new EventEmitter(), { kill }))
		);
		const spawnProcess = mock.fn(() => child);
		const spawnNative = createNativeSpawner({
			signalSource,
			spawnProcess,
		});

		const resultPromise = spawnNative('/native/bin/ccusage', ['statusline']);
		for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP', 'SIGQUIT']) {
			signalSource.emit(signal);
			signalSource.emit(signal);
		}
		child.emit('exit', 0, null);

		assert.deepEqual(await resultPromise, { signal: null, status: 0 });
		assert.deepEqual(
			kill.mock.calls.map((call) => call.arguments),
			[
				['SIGINT'],
				['SIGINT'],
				['SIGTERM'],
				['SIGTERM'],
				['SIGHUP'],
				['SIGHUP'],
				['SIGQUIT'],
				['SIGQUIT'],
			],
		);
		const spawnCall = spawnProcess.mock.calls[0];
		assert.ok(spawnCall);
		assert.deepEqual(spawnCall.arguments, [
			'/native/bin/ccusage',
			['statusline'],
			{ stdio: 'inherit' },
		]);
	});

	void it('removes signal listeners after the child exits', async () => {
		const signalSource = new EventEmitter();
		const kill = mock.fn(() => true);
		const child = /** @type {import('node:child_process').ChildProcess} */ (
			/** @type {unknown} */ (Object.assign(new EventEmitter(), { kill }))
		);
		const spawnNative = createNativeSpawner({
			signalSource,
			spawnProcess: () => child,
		});

		const resultPromise = spawnNative('/native/bin/ccusage', []);
		child.emit('exit', 7, 'SIGTERM');

		assert.deepEqual(await resultPromise, { signal: 'SIGTERM', status: 7 });
		assert.deepEqual(
			['SIGINT', 'SIGTERM', 'SIGHUP', 'SIGQUIT'].map((signal) =>
				signalSource.listenerCount(signal),
			),
			[0, 0, 0, 0],
		);
		signalSource.emit('SIGTERM');
		assert.equal(kill.mock.callCount(), 0);
	});

	void it('removes signal listeners after a child error', async () => {
		const signalSource = new EventEmitter();
		const kill = mock.fn(() => true);
		const child = /** @type {import('node:child_process').ChildProcess} */ (
			/** @type {unknown} */ (Object.assign(new EventEmitter(), { kill }))
		);
		const spawnNative = createNativeSpawner({
			signalSource,
			spawnProcess: () => child,
		});
		const error = new Error('spawn failed');

		const resultPromise = spawnNative('/native/bin/ccusage', []);
		child.emit('error', error);

		assert.deepEqual(await resultPromise, {
			error,
			signal: null,
			status: null,
		});
		assert.deepEqual(
			['SIGINT', 'SIGTERM', 'SIGHUP', 'SIGQUIT'].map((signal) =>
				signalSource.listenerCount(signal),
			),
			[0, 0, 0, 0],
		);
		signalSource.emit('SIGINT');
		assert.equal(kill.mock.callCount(), 0);
	});
});
