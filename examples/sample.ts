// Retry with exponential backoff and jitter.
interface RetryPolicy {
  attempts: number;
  baseMs: number;
}

type Task<T> = () => Promise<T>;

async function withRetry<T>(task: Task<T>, policy: RetryPolicy): Promise<T> {
  let lastError: unknown = null;
  for (let attempt = 0; attempt < policy.attempts; attempt++) {
    try {
      return await task();
    } catch (error) {
      lastError = error;
      const delay = policy.baseMs * 2 ** attempt + Math.random() * 50;
      await new Promise((resolve) => setTimeout(resolve, delay));
    }
  }
  throw lastError;
}

export { withRetry, type RetryPolicy };
