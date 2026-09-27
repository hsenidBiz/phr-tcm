// The guide's closing section: who built the app, who tested it, and who
// made it possible for the teams to use it.

export type Acknowledgements = {
  team: string;
  developer: string;
  testers: string[];
  thanks: { name: string; for: string }[];
};

export const acknowledgements: Acknowledgements = {
  team: "Built by the Innovation Team.",
  developer: "Avin Alwis",
  testers: [
    "Ishani Dasanayake",
    "Naveen Warnakulasuriya",
    "Sachila Manamperi",
    "Vishwa Warnakulasuriya",
    "Hansani Gunasekara",
  ],
  thanks: [
    {
      name: "Ayub Sourjah",
      for: "for the feedback, and for the opportunity to bring Test Case Manager to the teams.",
    },
  ],
};
