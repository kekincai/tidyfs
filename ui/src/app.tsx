import {useApp} from 'ink';
import {useCallback, useEffect, useState} from 'react';
import type {Logger} from 'pino';
import type {EngineClient} from './engine/client.js';
import type {Hello, RootApply, RootScan} from './engine/protocol.js';
import {Applying} from './screens/Applying.js';
import {Review} from './screens/Review.js';
import {Scanning} from './screens/Scanning.js';
import {Setup} from './screens/Setup.js';
import {Summary} from './screens/Summary.js';
import {mergeDrives, type Settings, type Target} from './state.js';

type View =
	| {kind: 'setup'}
	| {kind: 'scanning'; roots: string[]}
	| {kind: 'review'; scanId: number; results: RootScan[]}
	| {kind: 'applying'; scanId: number; results: RootScan[]; roots: number[]}
	| {kind: 'summary'; applied: RootApply[]; cancelled: boolean};

export function App({engine, log}: {engine: EngineClient; log: Logger}) {
	const {exit} = useApp();
	const [hello, setHello] = useState<Hello | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [targets, setTargets] = useState<Target[]>([]);
	const [settings, setSettings] = useState<Settings>({task: 'empty', mode: 'keep-endpoints'});
	const [view, setView] = useState<View>({kind: 'setup'});

	const refreshDrives = useCallback(() => {
		engine
			.request({cmd: 'drives'}, 'drives')
			.then(({drives}) => setTargets(previous => mergeDrives(previous, drives)))
			.catch((caught: Error) => setError(caught.message));
	}, [engine]);

	useEffect(() => {
		engine
			.request({cmd: 'hello'}, 'hello')
			.then(info => {
				log.info({engine: info.version, config: info.configPath}, 'engine ready');
				setHello(info);
				setSettings(previous => ({...previous, mode: info.flattenMode}));
			})
			.catch((caught: Error) => setError(`无法连接引擎：${caught.message}`));
		refreshDrives();
	}, []);

	const quit = () => {
		log.info('quit');
		engine.close();
		exit();
	};

	const backToSetup = () => {
		if (view.kind === 'review') engine.stream({cmd: 'forget', scan: view.scanId}, () => {});
		setView({kind: 'setup'});
	};

	switch (view.kind) {
		case 'setup':
			return (
				<Setup
					engine={engine}
					hello={hello}
					error={error}
					targets={targets}
					setTargets={setTargets}
					settings={settings}
					setSettings={setSettings}
					refreshDrives={refreshDrives}
					onQuit={quit}
					onStart={roots => {
						log.info({task: settings.task, mode: settings.mode, roots}, 'scan');
						setView({kind: 'scanning', roots});
					}}
				/>
			);
		case 'scanning':
			return (
				<Scanning
					engine={engine}
					settings={settings}
					roots={view.roots}
					onBack={backToSetup}
					onDone={(scanId, results) => setView({kind: 'review', scanId, results})}
				/>
			);
		case 'review':
			return (
				<Review
					engine={engine}
					settings={settings}
					scanId={view.scanId}
					results={view.results}
					onBack={backToSetup}
					onQuit={quit}
					onApply={roots => {
						log.info({scan: view.scanId, roots}, 'apply');
						setView({kind: 'applying', scanId: view.scanId, results: view.results, roots});
					}}
				/>
			);
		case 'applying':
			return (
				<Applying
					engine={engine}
					settings={settings}
					scanId={view.scanId}
					results={view.results}
					roots={view.roots}
					onBack={backToSetup}
					onDone={(applied, cancelled) => setView({kind: 'summary', applied, cancelled})}
				/>
			);
		case 'summary':
			return (
				<Summary
					settings={settings}
					hello={hello}
					applied={view.applied}
					cancelled={view.cancelled}
					onQuit={quit}
					onBack={() => {
						refreshDrives();
						setView({kind: 'setup'});
					}}
				/>
			);
	}
}
