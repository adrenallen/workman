export const feedbackContentToken = '{feedback}';
export const feedbackTitleToken = '{title}';

export const defaultRecordedFeedbackAgentPrompt = `The user recorded some feedback as follows:

# Feedback

{feedback}`;

export interface RecordedFeedbackPromptFrame {
  before: string;
  after: string;
}

/** Split the wrapper around the point where ordered transcript and image blocks are inserted. */
export function recordedFeedbackPromptFrame(
  template: string,
  title: string
): RecordedFeedbackPromptFrame {
  const marker = template.indexOf(feedbackContentToken);
  if (marker < 0) {
    return { before: template.split(feedbackTitleToken).join(title), after: '' };
  }
  return {
    before: template.slice(0, marker).split(feedbackTitleToken).join(title),
    after: template.slice(marker + feedbackContentToken.length).split(feedbackTitleToken).join(title)
  };
}

/** Render the wrapper for clipboard/file delivery, appending feedback when its token was removed. */
export function renderRecordedFeedbackPrompt(
  template: string,
  title: string,
  feedback: string
): string {
  const frame = recordedFeedbackPromptFrame(template, title);
  return [frame.before.trim(), feedback.trim(), frame.after.trim()]
    .filter(Boolean)
    .join('\n\n');
}
