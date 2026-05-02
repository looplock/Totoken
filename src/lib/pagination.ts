export type PageToken = number | 'ellipsis-left' | 'ellipsis-right';

export function buildPageTokens(page: number, totalPages: number): PageToken[] {
  if (totalPages <= 7) {
    return Array.from({ length: totalPages }, (_, index) => index + 1);
  }

  const pages: PageToken[] = [1];
  if (page > 3) {
    pages.push('ellipsis-left');
  }

  const start = Math.max(2, page - 1);
  const end = Math.min(totalPages - 1, page + 1);
  for (let current = start; current <= end; current += 1) {
    pages.push(current);
  }

  if (page < totalPages - 2) {
    pages.push('ellipsis-right');
  }

  pages.push(totalPages);
  return pages;
}
