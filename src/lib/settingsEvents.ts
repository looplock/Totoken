export const UI_PREFERENCES_UPDATED_EVENT = 'totoken:ui-preferences-updated';

export type UiPreferencesUpdatedDetail = {
  notifications: boolean;
  localizedTokenUnits: boolean;
};

export function emitUiPreferencesUpdated(detail: UiPreferencesUpdatedDetail) {
  window.dispatchEvent(
    new CustomEvent<UiPreferencesUpdatedDetail>(UI_PREFERENCES_UPDATED_EVENT, { detail }),
  );
}
