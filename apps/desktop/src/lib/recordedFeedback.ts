import type { ProcessView } from './daemon';

export type RecordedFeedbackStatus = 'recording' | 'transcribing' | 'ready' | 'failed';

export interface RecordedFeedbackTranscriptSegment {
  start_ms: number;
  end_ms: number;
  text: string;
}

export type RecordedFeedbackBlock =
  | { kind: 'text'; text: string; start_ms: number; end_ms: number }
  | { kind: 'image'; snapshot_id: number };

export interface RecordedFeedbackSnapshot {
  id: number;
  feedback_id: number;
  ordinal: number;
  anchor_ms: number;
  anchor_samples: number;
  invoked_at_ms: number;
  completed_at_ms: number;
  image_path: string;
  caption: string;
  width: number;
  height: number;
  sha256: string;
}

export interface RecordedFeedbackDelivery {
  id: number;
  feedback_id: number;
  target_kind: 'agent' | 'scratchpad' | 'clipboard';
  target_id: number | null;
  target_name?: string | null;
  feedback_revision?: number | null;
  status: 'pending' | 'sent' | 'queued' | 'unverified' | 'failed';
  packet_path: string | null;
  error_message: string | null;
  created_at: number;
  updated_at: number;
}

export interface RecordedFeedbackSummary {
  id: number;
  project_id: number;
  title: string;
  status: RecordedFeedbackStatus;
  revision: number;
  duration_ms: number;
  snapshot_count: number;
  archived: boolean;
  error_code: string | null;
  created_at: number;
  updated_at: number;
}

export type RecordedFeedbackView = 'active' | 'archived';

export function recordedFeedbackForView(
  feedback: readonly RecordedFeedbackSummary[],
  view: RecordedFeedbackView,
  query = ''
): RecordedFeedbackSummary[] {
  const archived = view === 'archived';
  const needle = query.trim().toLocaleLowerCase();
  return feedback
    .filter((item) => item.archived === archived)
    .filter((item) => {
      if (!needle) return true;
      return `#${item.id} ${item.title} ${feedbackStatusLabel(item.status)} ${item.error_code ?? ''}`
        .toLocaleLowerCase()
        .includes(needle);
    })
    .sort((left, right) => right.updated_at - left.updated_at || right.id - left.id);
}

export interface RecordedFeedback extends Omit<RecordedFeedbackSummary, 'snapshot_count'> {
  append_state?: { duration_ms: number; audio_path: string | null; next_ordinal: number } | null;
  audio_path: string | null;
  transcript: RecordedFeedbackTranscriptSegment[];
  blocks: RecordedFeedbackBlock[];
  snapshots: RecordedFeedbackSnapshot[];
  deliveries: RecordedFeedbackDelivery[];
  lease_owner: string | null;
  lease_expires_at: number | null;
}

export function feedbackDeliveryLabel(delivery: RecordedFeedbackDelivery): string {
  if (delivery.status === 'sent') return delivery.target_kind === 'clipboard' ? 'Copied' : 'Sent';
  if (delivery.status === 'failed') return 'Failed';
  if (delivery.status === 'queued') return 'Queued';
  return 'Unconfirmed';
}

export function feedbackSendSummary(deliveries: readonly RecordedFeedbackDelivery[]): string {
  const sends = deliveries.filter((delivery) => delivery.target_kind !== 'clipboard');
  const sent = sends.filter((delivery) => delivery.status === 'sent');
  if (sent.length) return `Sent ${sent.length} time${sent.length === 1 ? '' : 's'}`;
  if (sends.some((delivery) => delivery.status !== 'failed')) return 'Delivery unconfirmed';
  return 'Not sent yet';
}

export interface FeedbackStartResult {
  feedback: RecordedFeedback;
  media_dir: string;
}

export interface FeedbackPacketResult {
  markdown: string;
  packet_path: string;
  delivery: RecordedFeedbackDelivery;
}

export interface NativeFeedbackPreflight {
  supported: boolean;
  platform: string;
  microphone_available: boolean;
  screen_capture_authorized: boolean;
  display_available: boolean;
  screen_capture_available: boolean;
  model_installed: boolean;
  model_name: string;
  model_size_bytes: number;
  model_path: string;
  message: string | null;
}

export interface NativeFeedbackSession {
  feedback_id: number;
  project_id: number;
  started_at_ms: number;
  elapsed_ms: number;
  audio_samples: number;
  sample_rate: number;
  snapshot_count: number;
  paused: boolean;
  muted: boolean;
  input_device_id: string;
  input_device_name: string;
  phase: 'recording' | 'stopping' | 'transcribing' | 'finished' | 'failed';
  error: string | null;
}

export interface NativeFeedbackAudioInput {
  id: string;
  name: string;
  is_default: boolean;
}

export interface NativeFeedbackAudioInputs {
  devices: NativeFeedbackAudioInput[];
  selected_id: string;
}

export interface NativeFeedbackSnapshot {
  feedback_id: number;
  project_id: number;
  display_index: number;
  ordinal: number;
  anchor_ms: number;
  anchor_samples: number;
  invoked_at_ms: number;
  completed_at_ms: number;
  image_path: string;
  sha256: string;
}

export interface NativeFeedbackFinished {
  feedback_id: number;
  project_id: number;
  duration_ms: number;
  audio_path: string | null;
}

export interface NativeFeedbackTranscript {
  feedback_id: number;
  project_id: number;
  segments: RecordedFeedbackTranscriptSegment[];
}

export function agentCanReceiveFeedback(process: ProcessView): boolean {
  return process.kind === 'agent'
    && process.status === 'running'
    && ['idle', 'needs_input', 'waiting'].includes(process.agent_state.state);
}

/** A narrower first-turn gate that can use a daemon's fast, stable-composer signal. */
export function agentCanReceiveInitialTurn(process: ProcessView): boolean {
  if (process.kind !== 'agent' || process.status !== 'running') return false;
  if (process.agent_state.composer_input_ready !== undefined) {
    return process.agent_state.composer_input_ready;
  }
  // Older daemons do not expose the fast signal. Keep their conservative behavior, but never
  // treat a permission or authentication prompt as the new agent's empty composer.
  return ['idle', 'waiting'].includes(process.agent_state.state);
}

export function agentFeedbackAvailability(process: ProcessView): string {
  if (process.kind !== 'agent') return 'Not an agent';
  if (process.status !== 'running') return 'Not running';
  if (!agentCanReceiveFeedback(process)) return 'Working — wait or send to a new agent';
  return process.agent_state.state === 'needs_input' ? 'Needs input' : 'Ready';
}

export function feedbackStatusLabel(status: RecordedFeedbackStatus): string {
  if (status === 'recording') return 'Recording';
  if (status === 'transcribing') return 'Transcribing';
  if (status === 'failed') return 'Needs attention';
  return 'Ready';
}

export function feedbackDuration(milliseconds: number): string {
  const seconds = Math.max(0, Math.floor(milliseconds / 1_000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}
