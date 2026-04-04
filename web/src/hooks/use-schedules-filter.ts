import { useMemo } from 'react';
import type { ScheduleInfo } from '@/api/types';

type StatusFilter = 'all' | 'active' | 'paused';

interface StatusCounts {
  all: number;
  active: number;
  paused: number;
}

export function useSchedulesFilter(
  schedules: ScheduleInfo[],
  searchQuery: string,
  statusFilter: StatusFilter,
) {
  const normalizedSearch = searchQuery.trim().toLowerCase();

  const counts = useMemo<StatusCounts>(() => {
    const statusCounts = { all: 0, active: 0, paused: 0 };
    for (const schedule of schedules) {
      statusCounts.all += 1;
      if (schedule.enabled) {
        statusCounts.active += 1;
      } else {
        statusCounts.paused += 1;
      }
    }
    return statusCounts;
  }, [schedules]);

  const filteredSchedules = useMemo(() => {
    return schedules.filter((schedule) => {
      if (normalizedSearch && !schedule.name.toLowerCase().includes(normalizedSearch)) {
        return false;
      }
      if (statusFilter === 'active' && !schedule.enabled) return false;
      if (statusFilter === 'paused' && schedule.enabled) return false;
      return true;
    });
  }, [schedules, normalizedSearch, statusFilter]);

  return { filteredSchedules, statusCounts: counts };
}
