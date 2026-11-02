export function policyTimeConditions(start: string, end: string): Record<string, Record<string, string[]>> | undefined {
  const from = start ? Date.parse(start) : undefined;
  const until = end ? Date.parse(end) : undefined;
  if ((from !== undefined && !Number.isFinite(from)) || (until !== undefined && !Number.isFinite(until)) ||
      (from !== undefined && until !== undefined && from >= until)) {
    throw new Error("有効な開始・終了日時を指定してください。終了日時は開始日時より後にしてください。");
  }
  if (from === undefined && until === undefined) return undefined;
  return {
    ...(from === undefined ? {} : { DateGreaterThanEquals: { "aws:CurrentTime": [new Date(from).toISOString()] } }),
    ...(until === undefined ? {} : { DateLessThan: { "aws:CurrentTime": [new Date(until).toISOString()] } }),
  };
}
