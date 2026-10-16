const p2 = (n: number) => String(n).padStart(2, "0");

/** Default file name of a capture export, in local time: „openbms-mitschnitt-20261016-100512.txt“. */
export const captureName = (d: Date) =>
  `openbms-mitschnitt-${d.getFullYear()}${p2(d.getMonth() + 1)}${p2(d.getDate())}-${p2(d.getHours())}${p2(d.getMinutes())}${p2(d.getSeconds())}.txt`;
