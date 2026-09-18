import { createDeepSeek } from "@ai-sdk/deepseek";
import { useChat } from "@ai-sdk/vue";
import { invoke } from "@tauri-apps/api/core";
import { info } from "@tauri-apps/plugin-log";
import { DirectChatTransport, lastAssistantMessageIsCompleteWithApprovalResponses } from "ai";
import { createBookAgent, MODEL_ID, savedEdits } from "~/agent/agent";
import { modelFetch } from "~/agent/fetch";
import { createJev } from "~/agent/jev";

/**
 * The chat with the book agent for one saved project. Messages live only in
 * memory. Each write asks the user first; `approve` answers, and the chat
 * resumes on its own once every pending write has an answer.
 *
 * `onBookChanged` runs after each turn in which Rust saved a write, so the
 * caller can reload the book on screen from Rust rather than trust the
 * model's account.
 */
export function useBookAgent(projectId: number, onBookChanged: () => Promise<void>) {
  const deepseek = createDeepSeek({
    // Never leaves the webview: `modelFetch` sends Rust only the path and body.
    apiKey: "held-by-rust",
    fetch: modelFetch("deepseek"),
  });
  const jev = createJev({
    fetch: modelFetch("jev"),
    log: (decision) => void info(JSON.stringify({ jev: decision })),
  });
  const agent = createBookAgent(projectId, { model: deepseek(MODEL_ID), invoke, jev });

  const refreshed = new Set<string>();
  const chat = useChat({
    transport: new DirectChatTransport({ agent }),
    sendAutomaticallyWhen: lastAssistantMessageIsCompleteWithApprovalResponses,
    onFinish: ({ messages }) => {
      const fresh = savedEdits(messages).filter((id) => !refreshed.has(id));
      if (fresh.length === 0) return;
      for (const id of fresh) refreshed.add(id);
      void onBookChanged();
    },
  });

  function approve(approvalId: string, approved: boolean) {
    void chat.addToolApprovalResponse({ id: approvalId, approved });
  }

  return { ...chat, approve };
}
