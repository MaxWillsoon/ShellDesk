import { expect, type Locator, type Page, test } from '@playwright/test';

async function expectAlertInsideDialog(page: Page, dialog: Locator, alert: Locator, text: string) {
  await expect(dialog).toBeVisible();
  await expect(alert).toContainText(text);

  const [dialogBox, alertBox] = await Promise.all([
    dialog.boundingBox(),
    alert.boundingBox(),
  ]);

  expect(dialogBox, 'dialog should have a rendered box').not.toBeNull();
  expect(alertBox, 'alert should have a rendered box').not.toBeNull();

  expect(alertBox!.x).toBeGreaterThanOrEqual(dialogBox!.x);
  expect(alertBox!.y).toBeGreaterThanOrEqual(dialogBox!.y);
  expect(alertBox!.x + alertBox!.width).toBeLessThanOrEqual(dialogBox!.x + dialogBox!.width);
  expect(alertBox!.y + alertBox!.height).toBeLessThanOrEqual(dialogBox!.y + dialogBox!.height);

  const alertIsTopmost = await alert.evaluate((node) => {
    const rect = node.getBoundingClientRect();
    const topElement = document.elementFromPoint(
      rect.left + rect.width / 2,
      rect.top + rect.height / 2,
    );
    return node === topElement || node.contains(topElement);
  });

  expect(alertIsTopmost, 'alert center should not be covered by another element').toBe(true);
}

test('MySQL create-table backend errors stay visible inside the modal', async ({ page }) => {
  await page.goto('/tests/ui/database-error-harness.html?component=mysql');

  await page.getByTestId('mysql-connect-submit').click();
  await expect(page.getByTestId('mysql-create-table-open')).toBeVisible();

  await page.getByTestId('mysql-create-table-open').click();
  const dialog = page.getByTestId('mysql-create-table-dialog');
  await expect(dialog).toBeVisible();

  await page.getByTestId('mysql-create-table-name').fill('broken_table');
  const columnName = page.getByTestId('mysql-create-table-column-name').first();
  await columnName.fill('id');
  await expect(columnName).toHaveValue('id');
  await page.getByTestId('mysql-create-table-execute').click();

  await expectAlertInsideDialog(
    page,
    dialog,
    page.getByTestId('mysql-dialog-error'),
    'mock create table failure',
  );
});

test('Redis destructive action errors stay visible inside the confirmation modal', async ({ page }) => {
  await page.goto('/tests/ui/database-error-harness.html?component=redis');

  await page.getByTestId('redis-connect-submit').click();
  await page.getByTestId('redis-key-row').click();
  await expect(page.getByTestId('redis-delete-key-open')).toBeVisible();

  await page.getByTestId('redis-delete-key-open').click();
  const dialog = page.getByTestId('redis-confirm-dialog');
  await expect(dialog).toBeVisible();

  await page.getByTestId('redis-confirm-execute').click();

  await expectAlertInsideDialog(
    page,
    dialog,
    page.getByTestId('redis-confirm-error'),
    'mock redis delete failure',
  );
});
