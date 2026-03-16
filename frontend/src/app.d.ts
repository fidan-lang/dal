import type { User } from "$lib/api";

declare global {
  namespace App {
    interface Locals {
      user: User | null;
    }
  }
}

export {};
