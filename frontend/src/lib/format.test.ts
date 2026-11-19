import { describe, expect, it } from "vitest";
import { bpToPercent, formatMinutes, formatMoney, percentToBp } from "./format";

describe("format", () => {
  it("montants en centimes", () => {
    expect(formatMoney(133500)).toBe("1 335,00 €");
    expect(formatMoney(5)).toBe("0,05 €");
    expect(formatMoney(-123456789)).toBe("-1 234 567,89 €");
  });

  it("durées", () => {
    expect(formatMinutes(150)).toBe("2h 30min");
    expect(formatMinutes(30)).toBe("30min");
    expect(formatMinutes(420)).toBe("7h");
  });

  it("taux de TVA", () => {
    expect(bpToPercent(2000)).toBe("20");
    expect(bpToPercent(550)).toBe("5,5");
    expect(percentToBp("5,5")).toBe(550);
    expect(percentToBp("20")).toBe(2000);
    expect(percentToBp("abc")).toBeNull();
    expect(percentToBp("1,234")).toBeNull();
  });
});

import { durationInput, parseDuration, parseMoney, parseQty, moneyInput } from "./format";

describe("saisies", () => {
  it("montants", () => {
    expect(parseMoney("80")).toBe(8000);
    expect(parseMoney("1 234,5")).toBe(123450);
    expect(parseMoney("12,345")).toBeNull();
    expect(moneyInput(8000)).toBe("80");
    expect(moneyInput(123450)).toBe("1234,50");
  });
  it("durées", () => {
    expect(parseDuration("7")).toBe(420);
    expect(parseDuration("7,5")).toBe(450);
    expect(parseDuration("2h30")).toBe(150);
    expect(parseDuration("2:30")).toBe(150);
    expect(parseDuration("45min")).toBe(45);
    expect(parseDuration("abc")).toBeNull();
    expect(durationInput(150)).toBe("2h30");
  });
  it("quantités", () => {
    expect(parseQty("1,5")).toBe(1500);
    expect(parseQty("14")).toBe(14000);
    expect(parseQty("-1")).toBeNull();
  });
});
